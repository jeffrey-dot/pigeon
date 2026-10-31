//! Session host: owns the PTYs and outlives every client window.
//!
//! One tokio runtime, one task per pane (that task is the only owner of the
//! pane's terminal state), one task per client. A client disconnecting only drops
//! its task — the PTYs and the layout keep running, which is the point of pigeon.

use std::collections::HashMap;
use std::fs::Permissions;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};

use pigeon_core::error::PgError;
use pigeon_core::layout::{Axis, Layout, LayoutNode};
use pigeon_core::proto::{self, Message, Node, SplitAxis, SplitNode, PROTO_VERSION};
use pigeon_core::session::PaneState;
use pigeon_core::term::Palette;

/// Capabilities this build offers (protocol §5).
const SERVER_FEATURES: &[&str] = &["delta", "scrollback", "osc7501"];
const SCROLLBACK: usize = 10_000;
/// At most one delta per pane per frame. A burst of output (`cat`, a build log) arrives in
/// many small PTY reads, and one delta each would repaint the whole screen dozens of times
/// per frame and flood an ssh link; a lone keystroke still goes out immediately.
const FRAME: Duration = Duration::from_millis(16);
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Pane ids are unique for the whole process, not per session.
static NEXT_PANE: AtomicU32 = AtomicU32::new(1);
/// Connection ids the host hands out in `hello_ack` (protocol §3.1).
static NEXT_CLIENT: AtomicU32 = AtomicU32::new(1);

/// Metadata a pane reports about itself (program-driven, not guessed).
#[derive(Clone, Default)]
struct PaneMeta {
    cwd: Option<String>,
    title: Option<String>,
}

/// How a session's box is picked when more than one client is attached (protocol §6).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SizePolicy {
    /// Whoever last focused, typed or resized owns the size.
    Latest,
    /// The smallest box any attached client asked for, so nothing is cut off.
    Smallest,
    /// Only the client that first attached may resize.
    Locked,
}

impl SizePolicy {
    fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("smallest") => Self::Smallest,
            Some("locked") => Self::Locked,
            _ => Self::Latest,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Smallest => "smallest",
            Self::Locked => "locked",
        }
    }
}

/// The box one attached client needs for its own grid.
struct Subscriber {
    cols: u16,
    rows: u16,
}

/// One session: one layout tree plus the panes its leaves point at.
struct Session {
    layout: Layout,
    panes: HashMap<u32, mpsc::Sender<Cmd>>,
    /// Shell used for spawns; its basename is the fallback pane label.
    shell: String,
    /// cwd the session was created with (OSC 7 overrides it per pane).
    cwd: String,
    /// The session box in cells, chosen by the size policy below.
    cols: u16,
    rows: u16,
    policy: SizePolicy,
    /// The client that decided `policy`; the only one `locked` listens to.
    owner: u32,
    /// The client `latest` currently honours.
    active: u32,
    /// Every attached client and the box it asked for.
    clients: HashMap<u32, Subscriber>,
}

type Meta = Arc<Mutex<HashMap<u32, PaneMeta>>>;

type Sessions = Arc<Mutex<HashMap<u32, Session>>>;
/// The colours the panes answer `OSC 4/10/11` with: harp's built-in dark theme until a client
/// sends `appearance`, then whatever that client last reported.
type PaletteSlot = Arc<std::sync::Mutex<Option<Palette>>>;

#[derive(Clone)]
enum Event {
    Delta {
        pane_id: u32,
        message: Message,
    },
    Exited {
        pane_id: u32,
        code: i32,
    },
    Layout {
        session_id: u32,
        message: Message,
    },
    /// A pane's size changed: who won the policy and how many clients watch (protocol §6).
    SizeState {
        session_id: u32,
        message: Message,
    },
    Status {
        pane_id: u32,
        message: Message,
    },
    /// Titles/cwd changed: clients should refresh `sessions`.
    SessionsChanged,
    /// The pane's grid changed size. Delta rows cannot say that, so watchers get a fresh
    /// snapshot instead (which also resets their `seq` base).
    Resized {
        pane_id: u32,
    },
}

enum Cmd {
    Input(Vec<u8>),
    /// Clipboard text; the pane decides on bracketing because only it knows the mode.
    Paste(String),
    Resize(u16, u16),
    /// The user has seen this status record.
    Ack(u32),
    /// Terminate the child (protocol `close_pane`).
    Kill,
    Snapshot {
        history: bool,
        reply: oneshot::Sender<Message>,
    },
    /// Deltas after `since`, or a snapshot when the backlog no longer has them all.
    Since {
        since: u32,
        reply: oneshot::Sender<Vec<Message>>,
    },
    /// Look `offset` lines into the history; the reply carries the view and the applied
    /// offset (protocol §3.3 `scroll`).
    Scroll {
        offset: usize,
        reply: oneshot::Sender<(Message, usize)>,
    },
    LatestStatus {
        reply: oneshot::Sender<Option<Message>>,
    },
    /// The client's colours changed: answer the panes' `OSC 4/10/11` queries with these.
    SetPalette(Option<Palette>),
}

/// How clients reach this host (docs/remote.md §2).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    /// Unix socket only: the local, always-on case.
    Socket,
    /// Socket *and* stdin/stdout: how a remote host is started over `ssh`, so the same
    /// process serves the ssh channel now and later bridges from the socket.
    Stdio,
    /// Bridge stdin/stdout to an already running host. `ssh host pigeon-host --proxy` makes
    /// the ssh channel the transport; this mode knows nothing about the protocol.
    Proxy,
}

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("pigeon-host: cannot start runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pigeon-host: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut socket = default_socket_path();
    let mut shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let mut transport = Transport::Socket;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = PathBuf::from(args.next().ok_or("--socket needs a path")?),
            "--shell" => shell = args.next().ok_or("--shell needs a path")?,
            "--stdio" => transport = Transport::Stdio,
            // Prints where the socket would be and exits: the client needs the exact path to
            // set up `ssh -L` without duplicating the rule (protocol §1).
            "--socket-path" => {
                println!("{}", default_socket_path().display());
                return Ok(());
            }
            "--proxy" => transport = Transport::Proxy,
            "-h" | "--help" => {
                println!("usage: pigeon-host [--socket PATH] [--shell PATH] [--stdio|--proxy]");
                println!("  --socket-path  print the default socket path and exit");
                println!("  --stdio  also serve one client on stdin/stdout (start a remote host)");
                println!("  --proxy  bridge stdin/stdout to a running host's socket");
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let (events, _) = broadcast::channel::<Event>(256);
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let meta: Meta = Arc::new(Mutex::new(HashMap::new()));
    // Before a client speaks, answer colour queries with harp's built-in dark theme.
    let palette: PaletteSlot = Arc::new(std::sync::Mutex::new(Some(Palette::default_dark())));

    if transport == Transport::Proxy {
        return proxy(&socket, &shell).await;
    }

    let listener = bind_private(&socket).await?;
    eprintln!("pigeon-host listening on {}", socket.display());

    if transport == Transport::Stdio {
        // One extra client on this process's stdio. When the ssh channel closes the client
        // goes away and the host keeps serving the socket: that is what makes a remote host
        // outlive the window that started it.
        let sessions = Arc::clone(&sessions);
        let events = events.clone();
        let meta = Arc::clone(&meta);
        let palette = Arc::clone(&palette);
        let shell = shell.clone();
        tokio::spawn(async move {
            let stdio = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
            if let Err(error) = handle_client(stdio, sessions, events, meta, palette, shell).await {
                eprintln!("pigeon-host: stdio client gone: {error}");
            }
        });
    }

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("pigeon-host: accept failed: {error}");
                continue;
            }
        };
        let sessions = Arc::clone(&sessions);
        let events = events.clone();
        let meta = Arc::clone(&meta);
        let palette = Arc::clone(&palette);
        let shell = shell.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, sessions, events, meta, palette, shell).await
            {
                eprintln!("pigeon-host: client gone: {error}");
            }
        });
    }
}

/// Takes over `socket` only if nobody answers on it: a live host keeps its socket and its
/// sessions, and this process exits instead (a stray start, a `--stdio` racing one).
///
/// The socket must never be reachable by other users, not even between `bind` and `chmod`.
/// Changing the umask needs `unsafe`, so the socket is bound inside a fresh 0700 directory
/// (unreachable for anyone else), chmodded there, and only then renamed into place.
async fn bind_private(socket: &std::path::Path) -> Result<UnixListener, String> {
    if std::fs::metadata(socket).is_ok_and(|meta| meta.file_type().is_socket()) {
        if UnixStream::connect(socket).await.is_ok() {
            return Err(format!("another pigeon-host is already serving {socket:?}"));
        }
        // Nobody listening: a crashed host left its socket behind.
        let _ = std::fs::remove_file(socket);
    }
    let parent = socket.parent().unwrap_or_else(|| std::path::Path::new("."));
    let staging = parent.join(format!(".pigeon-bind-{}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|error| format!("mkdir {staging:?}: {error}"))?;
    std::fs::set_permissions(&staging, Permissions::from_mode(0o700))
        .map_err(|error| format!("chmod {staging:?}: {error}"))?;
    let temporary = staging.join("pigeon.sock");
    let bound = (|| {
        let listener = UnixListener::bind(&temporary)
            .map_err(|error| format!("bind {temporary:?}: {error}"))?;
        std::fs::set_permissions(&temporary, Permissions::from_mode(0o700))
            .map_err(|error| format!("chmod {temporary:?}: {error}"))?;
        std::fs::rename(&temporary, socket)
            .map_err(|error| format!("rename to {socket:?}: {error}"))?;
        Ok(listener)
    })();
    let _ = std::fs::remove_file(&temporary);
    let _ = std::fs::remove_dir(&staging);
    bound
}

#[derive(Default)]
struct ClientState {
    /// Connection id this host assigned in `hello_ack`, unique per connection.
    client_id: u32,
    greeted: bool,
    /// Panes this connection gets live output for → negotiated `delta`.
    watched: HashMap<u32, bool>,
    /// The session whose layout this connection follows (one per window in M1).
    session: Option<u32>,
    /// Whether the handshake negotiated `delta` for this connection.
    delta_ok: bool,
    /// How far each pane is scrolled back for *this* window; 0 is the live screen. Two
    /// windows on one session scroll independently because this lives per connection.
    views: HashMap<u32, usize>,
    /// Frames a request produced beyond its one reply (an `attach.since` replay).
    outbox: Vec<Message>,
}

async fn handle_client<S>(
    stream: S,
    sessions: Sessions,
    events: broadcast::Sender<Event>,
    meta: Meta,
    palette: PaletteSlot,
    shell: String,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let client_id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    let result = client_loop(
        stream, &sessions, &events, &meta, &palette, &shell, client_id,
    )
    .await;
    // A closed window must stop counting towards the session's size policy, or
    // `smallest` would stay pinned to a box nobody can see any more.
    forget_client(&sessions, &events, client_id).await;
    result
}

#[allow(clippy::too_many_arguments)]
async fn client_loop<S>(
    stream: S,
    sessions: &Sessions,
    events: &broadcast::Sender<Event>,
    meta: &Meta,
    palette: &PaletteSlot,
    shell: &str,
    client_id: u32,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut inbox = Vec::new();
    let mut subscribed = events.subscribe();
    let mut state = ClientState {
        client_id,
        ..Default::default()
    };

    loop {
        tokio::select! {
            read = reader.read_buf(&mut inbox) => {
                let count = read.map_err(|error| error.to_string())?;
                if count == 0 {
                    return Ok(());
                }
                loop {
                    match proto::decode_frame(&inbox) {
                        Ok(Some((message, used))) => {
                            inbox.drain(..used);
                            if std::env::var_os("PIGEON_DEBUG").is_some() {
                                eprintln!("pigeon-host: <- {}", message.name());
                            }
                            if let Some(reply) =
                                dispatch(message, sessions, events, meta, palette, shell, &mut state)
                                    .await
                            {
                                let handshake = matches!(reply, Message::HelloAck { .. });
                                let attached_pane = match &reply {
                                    Message::Snapshot { pane_id, .. } => Some(*pane_id),
                                    _ => None,
                                };
                                send(&mut writer, &reply).await?;
                                if let Some(pane_id) = attached_pane {
                                    if let Some(status) = latest_status(sessions, pane_id).await {
                                        send(&mut writer, &status).await?;
                                    }
                                }
                                // Protocol §3.2: the session list follows the ack.
                                if handshake {
                                    if let Some(list) = dispatch(
                                        Message::Sessions { items: Vec::new() },
                                        sessions,
                                        events,
                                        meta,
                                        palette,
                                        shell,
                                        &mut state,
                                    )
                                    .await
                                    {
                                        send(&mut writer, &list).await?;
                                    }
                                }
                            }
                            // Outside the reply branch: a replay has no single reply.
                            for frame in std::mem::take(&mut state.outbox) {
                                send(&mut writer, &frame).await?;
                            }
                        }
                        Ok(None) => break,
                        Err(PgError::Unsupported) => {
                            let t_of = type_tag(&inbox).unwrap_or_default();
                            send(&mut writer, &Message::Unsupported { t_of }).await?;
                            inbox.clear();
                        }
                        Err(error) => {
                            let detail = format!("{error}");
                            send(&mut writer, &Message::Error { code: "protocol".into(), detail, id: None }).await?;
                            return Ok(());
                        }
                    }
                }
            }
            event = subscribed.recv() => {
                let Ok(event) = event else { continue };
                match event {
                    Event::Layout { session_id, message } => {
                        // Follow the session even after its last watched pane is
                        // gone, so closing the active pane still redraws the tree.
                        if state.session == Some(session_id) {
                            send(&mut writer, &message).await?;
                        }
                    }
                    Event::SizeState { session_id, message } => {
                        if state.session == Some(session_id) {
                            send(&mut writer, &message).await?;
                        }
                    }
                    Event::Exited { pane_id, code } => {
                        if state.watched.contains_key(&pane_id) {
                            send(&mut writer, &Message::PaneExited { pane_id, code }).await?;
                        }
                    }
                    Event::Status { pane_id, message } => {
                        if state.watched.contains_key(&pane_id) {
                            send(&mut writer, &message).await?;
                        }
                    }
                    Event::SessionsChanged => {
                        if let Some(list) = dispatch(
                            Message::Sessions { items: Vec::new() },
                            sessions,
                            events,
                            meta,
                            palette,
                            shell,
                            &mut state,
                        )
                        .await
                        {
                            send(&mut writer, &list).await?;
                        }
                    }
                    Event::Resized { pane_id } => {
                        if !state.watched.contains_key(&pane_id)
                            || state.views.get(&pane_id).copied().unwrap_or(0) > 0
                        {
                            continue;
                        }
                        if let Some(snapshot) = snapshot_of(sessions, pane_id, false).await {
                            send(&mut writer, &snapshot).await?;
                        }
                    }
                    Event::Delta { pane_id, message } => {
                        let Some(delta_ok) = state.watched.get(&pane_id).copied() else {
                            continue;
                        };
                        // A scrolled-back window is reading history: live repaints would
                        // tear the view it is looking at (protocol §3.3).
                        if state.views.get(&pane_id).copied().unwrap_or(0) > 0 {
                            continue;
                        }
                        if delta_ok {
                            send(&mut writer, &message).await?;
                        } else if let Some(snapshot) = snapshot_of(sessions, pane_id, false).await {
                            send(&mut writer, &snapshot).await?;
                        }
                    }
                }
            }
        }
    }
}

/// What a reconnecting client missed since `since`: deltas, or one snapshot.
async fn catch_up(sessions: &Sessions, pane_id: u32, since: u32) -> Option<Vec<Message>> {
    let sender = sender_of(sessions, pane_id).await?;
    let (tx, rx) = oneshot::channel();
    sender.send(Cmd::Since { since, reply: tx }).await.ok()?;
    rx.await.ok()
}

async fn snapshot_of(sessions: &Sessions, pane_id: u32, history: bool) -> Option<Message> {
    let sender = sender_of(sessions, pane_id).await?;
    let (tx, rx) = oneshot::channel();
    sender
        .send(Cmd::Snapshot { history, reply: tx })
        .await
        .ok()?;
    rx.await.ok()
}

/// The pane's current status, so a client that attaches later still sees it.
async fn latest_status(sessions: &Sessions, pane_id: u32) -> Option<Message> {
    let sender = sender_of(sessions, pane_id).await?;
    let (tx, rx) = oneshot::channel();
    sender.send(Cmd::LatestStatus { reply: tx }).await.ok()?;
    rx.await.ok().flatten()
}

async fn sender_of(sessions: &Sessions, pane_id: u32) -> Option<mpsc::Sender<Cmd>> {
    let map = sessions.lock().await;
    map.values()
        .find_map(|session| session.panes.get(&pane_id).cloned())
}

fn session_of(map: &HashMap<u32, Session>, pane_id: u32) -> Option<u32> {
    map.iter()
        .find(|(_, session)| session.panes.contains_key(&pane_id))
        .map(|(id, _)| *id)
}

fn layout_message(session_id: u32, session: &Session) -> Message {
    Message::Layout {
        session_id,
        tree: session
            .layout
            .describe()
            .map(to_proto_tree)
            .unwrap_or(Node::Leaf { leaf: 0 }),
        zoom: session.layout.zoomed(),
        focus: session.layout.focused(),
    }
}

fn to_proto_tree(node: LayoutNode) -> Node {
    match node {
        LayoutNode::Leaf(pane) => Node::Leaf { leaf: pane },
        LayoutNode::Split {
            id,
            axis,
            ratio,
            a,
            b,
        } => Node::Split {
            split: SplitNode {
                id,
                axis: match axis {
                    Axis::Vertical => SplitAxis::Vertical,
                    Axis::Horizontal => SplitAxis::Horizontal,
                },
                ratio,
                a: Box::new(to_proto_tree(*a)),
                b: Box::new(to_proto_tree(*b)),
            },
        },
    }
}

/// Hands `command` to a pane without blocking the caller (which holds the sessions lock)
/// and without losing it: a full queue gets the command from a task that waits its turn.
/// A lost `Resize` leaves the PTY at the wrong size, a lost `Kill` an orphaned process.
fn send_or_queue(sender: &mpsc::Sender<Cmd>, command: Cmd) {
    match sender.try_send(command) {
        Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
        Err(mpsc::error::TrySendError::Full(command)) => {
            let sender = sender.clone();
            tokio::spawn(async move {
                let _ = sender.send(command).await;
            });
        }
    }
}

/// One pane's box in cells: its share of the session box, clamped to the minimums the
/// VM needs (a 1×0 grid is not a terminal).
fn pane_size(session: &Session, rect: &pigeon_core::layout::Rect) -> (u16, u16) {
    let cols = ((rect.w * f32::from(session.cols)).round() as u16).max(2);
    let rows = ((rect.h * f32::from(session.rows)).round() as u16).max(1);
    (cols, rows)
}

/// Re-fits every pane to its rect and tells clients what the tree looks like.
fn apply_layout(session_id: u32, session: &mut Session, events: &broadcast::Sender<Event>) {
    for (pane_id, rect) in session.layout.rects() {
        let (cols, rows) = pane_size(session, &rect);
        if let Some(sender) = session.panes.get(&pane_id) {
            send_or_queue(sender, Cmd::Resize(cols, rows));
        }
    }
    let _ = events.send(Event::Layout {
        session_id,
        message: layout_message(session_id, session),
    });
    broadcast_size(session_id, session, events);
}

/// Protocol §6: every size change is announced with its `source` and the number of
/// clients, so each window can show who is in charge.
fn broadcast_size(session_id: u32, session: &Session, events: &broadcast::Sender<Event>) {
    let clients = session.clients.len().min(usize::from(u8::MAX)) as u8;
    if clients > 1 && std::env::var_os("PIGEON_DEBUG").is_some() {
        eprintln!(
            "pigeon-host: size_state session={session_id} clients={clients} source={} box={}x{}",
            session.policy.as_str(),
            session.cols,
            session.rows
        );
    }
    for (pane_id, rect) in session.layout.rects() {
        let (cols, rows) = pane_size(session, &rect);
        let _ = events.send(Event::SizeState {
            session_id,
            message: Message::SizeState {
                pane_id,
                cols,
                rows,
                source: session.policy.as_str().to_owned(),
                clients,
            },
        });
    }
}

/// The session box the current policy asks for, given what each client last reported.
fn negotiated_box(session: &Session) -> (u16, u16) {
    match session.policy {
        SizePolicy::Smallest => session
            .clients
            .values()
            .fold((u16::MAX, u16::MAX), |(cols, rows), client| {
                (cols.min(client.cols), rows.min(client.rows))
            }),
        // `locked` only lets the owner report, and `active` is pinned to it on attach.
        SizePolicy::Latest | SizePolicy::Locked => session
            .clients
            .get(&session.active)
            .map(|client| (client.cols, client.rows))
            .unwrap_or((session.cols, session.rows)),
    }
}

fn box_changed(session: &mut Session) -> bool {
    let (cols, rows) = negotiated_box(session);
    let changed = (cols, rows) != (session.cols, session.rows) && cols != u16::MAX;
    if changed {
        session.cols = cols;
        session.rows = rows;
    }
    changed
}

/// Protocol §6 `latest`: whoever last focused, typed or resized owns the size.
async fn note_activity(
    sessions: &Sessions,
    events: &broadcast::Sender<Event>,
    client_id: u32,
    pane_id: u32,
) {
    let mut map = sessions.lock().await;
    let Some(session_id) = session_of(&map, pane_id) else {
        return;
    };
    let Some(session) = map.get_mut(&session_id) else {
        return;
    };
    if session.policy != SizePolicy::Latest
        || session.active == client_id
        || !session.clients.contains_key(&client_id)
    {
        return;
    }
    session.active = client_id;
    if box_changed(session) {
        apply_layout(session_id, session, events);
    }
}

/// Drops a closed connection from every session and re-fits whatever the remaining
/// clients ask for.
async fn forget_client(sessions: &Sessions, events: &broadcast::Sender<Event>, client_id: u32) {
    let mut map = sessions.lock().await;
    for (session_id, session) in map.iter_mut() {
        if session.clients.remove(&client_id).is_none() {
            continue;
        }
        if session.active == client_id {
            session.active = session.clients.keys().copied().min().unwrap_or(0);
        }
        if box_changed(session) {
            apply_layout(*session_id, session, events);
        }
    }
}

async fn dispatch(
    message: Message,
    sessions: &Sessions,
    events: &broadcast::Sender<Event>,
    meta: &Meta,
    palette: &PaletteSlot,
    shell: &str,
    state: &mut ClientState,
) -> Option<Message> {
    if !state.greeted && !matches!(message, Message::Hello { .. }) {
        return Some(Message::Error {
            code: "protocol".into(),
            detail: "hello must be the first message".into(),
            id: None,
        });
    }
    match message {
        Message::Hello {
            proto: version,
            features,
            ..
        } => {
            state.greeted = true;
            if version >> 8 != PROTO_VERSION >> 8 {
                return Some(Message::HelloError {
                    reason: "version".into(),
                    detail: format!("need {PROTO_VERSION:#06x}"),
                });
            }
            let offered: Vec<String> = SERVER_FEATURES
                .iter()
                .filter(|feature| features.iter().any(|asked| asked == *feature))
                .map(|feature| (*feature).to_owned())
                .collect();
            state.delta_ok = offered.iter().any(|f| f == "delta");
            Some(Message::HelloAck {
                proto: PROTO_VERSION,
                server: format!("pigeon-host {}", env!("CARGO_PKG_VERSION")),
                features: offered,
                host_id: hostname(),
                client_id: state.client_id,
                instance: Some(instance().to_owned()),
            })
        }
        Message::Sessions { .. } => {
            let map = sessions.lock().await;
            let meta = meta.lock().await;
            Some(Message::Sessions {
                items: map
                    .iter()
                    .map(|(session_id, session)| {
                        let active = session.layout.focused();
                        let info = active.and_then(|pane| meta.get(&pane));
                        let raw_title = info.and_then(|info| info.title.clone());
                        // A window title can be a program (`vim`) or a prompt (`~/dev`): only
                        // the first is a name for the sidebar row.
                        let program = raw_title
                            .clone()
                            .filter(|title| !looks_like_a_prompt(title))
                            .unwrap_or_else(|| session.shell.clone());
                        proto::SessionInfo {
                            session_id: *session_id,
                            title: format!("session {session_id}"),
                            cwd: info
                                .and_then(|info| info.cwd.clone())
                                .unwrap_or_else(|| session.cwd.clone()),
                            panes: session.layout.pane_ids(),
                            active_pane: active,
                            active_title: Some(
                                raw_title.clone().unwrap_or_else(|| session.shell.clone()),
                            ),
                            active_program: Some(program),
                        }
                    })
                    .collect(),
            })
        }
        // The client tells us what it is drawing, so the programs inside the panes can pick
        // their light or dark theme (harp answers the same queries from its theme).
        Message::Appearance { fg, bg, ansi } => {
            let Some(colours) = fg
                .zip(bg)
                .and_then(|(fg, bg)| Palette::from_rgb(fg, bg, &ansi))
            else {
                // Not enough information (or a wrong-length palette): keep the old answer set
                // rather than inventing colours.
                return None;
            };
            if let Ok(mut slot) = palette.lock() {
                *slot = Some(colours);
            }
            let map = sessions.lock().await;
            for session in map.values() {
                for sender in session.panes.values() {
                    let _ = sender.try_send(Cmd::SetPalette(Some(colours)));
                }
            }
            None
        }
        Message::NewSession {
            id,
            cwd,
            shell: requested,
            ..
        } => {
            let shell = requested.unwrap_or_else(|| shell.to_owned());
            let launch_cwd = cwd.clone().unwrap_or_default();
            let Some((pane_id, sender)) = spawn_pane(
                events,
                meta,
                &shell,
                cwd,
                DEFAULT_COLS,
                DEFAULT_ROWS,
                current_palette(palette),
            ) else {
                return Some(Message::Error {
                    code: "internal".into(),
                    detail: "cannot spawn the shell".into(),
                    id: Some(id),
                });
            };
            let mut panes = HashMap::new();
            panes.insert(pane_id, sender);
            let session = Session {
                layout: Layout::new(pane_id),
                panes,
                shell: shell_label(&shell),
                cwd: launch_cwd,
                cols: DEFAULT_COLS,
                rows: DEFAULT_ROWS,
                policy: SizePolicy::Latest,
                owner: 0,
                active: 0,
                clients: HashMap::new(),
            };
            sessions.lock().await.insert(pane_id, session);
            Some(Message::Created {
                id,
                session_id: pane_id,
                pane_id,
            })
        }
        Message::Attach {
            id,
            pane_id,
            size_policy,
            history,
            since,
        } => {
            let session_id = {
                let map = sessions.lock().await;
                session_of(&map, pane_id)
            };
            let Some(session_id) = session_id else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            };
            // A reconnect (issue #4): replay what the client missed, which is usually a
            // few rows instead of the whole screen. Only for a client that takes deltas.
            let reply = match since.filter(|_| state.delta_ok) {
                Some(since) => catch_up(sessions, pane_id, since).await.map(|frames| {
                    state.outbox.extend(frames);
                    None
                }),
                None => snapshot_of(sessions, pane_id, history.unwrap_or(true))
                    .await
                    .map(Some),
            };
            let Some(snapshot) = reply else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            };
            state.watched.insert(pane_id, state.delta_ok);
            state.session = Some(session_id);
            // Protocol §6: the first `attach` decides the policy; later ones merely join.
            let mut map = sessions.lock().await;
            if let Some(session) = map.get_mut(&session_id) {
                if session.clients.is_empty() {
                    session.policy = SizePolicy::from_name(size_policy.as_deref());
                    session.owner = state.client_id;
                    session.active = state.client_id;
                }
                session.clients.insert(
                    state.client_id,
                    Subscriber {
                        cols: session.cols,
                        rows: session.rows,
                    },
                );
                if session.policy == SizePolicy::Locked {
                    session.active = session.owner;
                }
                // The tree follows the snapshot, so a reconnecting window redraws
                // the same split geometry and learns who owns the size.
                let _ = events.send(Event::Layout {
                    session_id,
                    message: layout_message(session_id, session),
                });
                broadcast_size(session_id, session, events);
            }
            // `latest_status` takes the sessions lock again.
            drop(map);
            if snapshot.is_none() {
                // The caught-up client still needs the status it may have missed.
                if let Some(status) = latest_status(sessions, pane_id).await {
                    state.outbox.push(status);
                }
            }
            snapshot
        }
        Message::SplitPane {
            id,
            pane_id,
            axis,
            ratio,
        } => {
            let axis = match axis {
                SplitAxis::Vertical => Axis::Vertical,
                SplitAxis::Horizontal => Axis::Horizontal,
            };
            let mut map = sessions.lock().await;
            let Some(session_id) = session_of(&map, pane_id) else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            };
            let session = map.get_mut(&session_id)?;
            let (cols, rows) = match axis {
                Axis::Vertical => ((session.cols / 2).max(2), session.rows),
                Axis::Horizontal => (session.cols, (session.rows / 2).max(1)),
            };
            let Some((new_pane, sender)) = spawn_pane(
                events,
                meta,
                shell,
                None,
                cols,
                rows,
                current_palette(palette),
            ) else {
                return Some(Message::Error {
                    code: "internal".into(),
                    detail: "cannot spawn the shell".into(),
                    id: Some(id),
                });
            };
            if session
                .layout
                .split(pane_id, new_pane, axis, ratio, session.cols, session.rows)
                .is_err()
            {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            }
            session.panes.insert(new_pane, sender);
            // The new pane takes focus, like every terminal's split.
            session.layout.focus(new_pane);
            apply_layout(session_id, session, events);
            None
        }
        Message::ResizeSplit {
            id,
            split_id,
            ratio,
        } => {
            let mut map = sessions.lock().await;
            let Some((session_id, session)) = map
                .iter_mut()
                .find(|(_, session)| session.layout.split_of_any(split_id))
            else {
                return Some(Message::Error {
                    code: "bad_request".into(),
                    detail: format!("split {split_id}"),
                    id: Some(id),
                });
            };
            if session
                .layout
                .set_ratio(split_id, ratio, session.cols, session.rows)
                .is_err()
            {
                return Some(Message::Error {
                    code: "bad_request".into(),
                    detail: format!("split {split_id}"),
                    id: Some(id),
                });
            }
            let session_id = *session_id;
            apply_layout(session_id, session, events);
            None
        }
        Message::Focus { pane_id } => {
            let mut map = sessions.lock().await;
            let session_id = session_of(&map, pane_id)?;
            let session = map.get_mut(&session_id)?;
            session.layout.focus(pane_id);
            // `latest`: focusing is what makes a window the one that owns the size.
            if session.policy != SizePolicy::Locked
                && session.clients.contains_key(&state.client_id)
            {
                session.active = state.client_id;
                box_changed(session);
            }
            apply_layout(session_id, session, events);
            None
        }
        Message::Keys { pane_id, bytes } => {
            if let Some(sender) = sender_of(sessions, pane_id).await {
                let _ = sender.send(Cmd::Input(bytes)).await;
            }
            note_activity(sessions, events, state.client_id, pane_id).await;
            None
        }
        Message::Paste { pane_id, text } => {
            if let Some(sender) = sender_of(sessions, pane_id).await {
                let _ = sender.send(Cmd::Paste(text)).await;
            }
            note_activity(sessions, events, state.client_id, pane_id).await;
            None
        }
        Message::Scroll { pane_id, offset } => {
            let current = state.views.get(&pane_id).copied().unwrap_or(0);
            let wanted = if offset == 0 {
                0
            } else {
                (current as i64 - i64::from(offset)).max(0) as usize
            };
            let Some(sender) = sender_of(sessions, pane_id).await else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: None,
                });
            };
            let (tx, rx) = oneshot::channel();
            sender
                .send(Cmd::Scroll {
                    offset: wanted,
                    reply: tx,
                })
                .await
                .ok()?;
            let (message, applied) = rx.await.ok()?;
            state.views.insert(pane_id, applied);
            Some(message)
        }
        Message::ResizePane {
            pane_id,
            cols,
            rows,
        } => {
            let mut map = sessions.lock().await;
            let session_id = session_of(&map, pane_id)?;
            let client_id = state.client_id;
            let session = map.get_mut(&session_id)?;
            // The box this client's grid implies for the whole session.
            let rect = session
                .layout
                .rects()
                .into_iter()
                .find(|(id, _)| *id == pane_id)
                .map(|(_, rect)| rect);
            let rect = rect?;
            let (cols, rows) = (
                if rect.w > 0.0 {
                    ((f32::from(cols) / rect.w).round() as u16).max(2)
                } else {
                    session.cols
                },
                if rect.h > 0.0 {
                    ((f32::from(rows) / rect.h).round() as u16).max(1)
                } else {
                    session.rows
                },
            );
            if let Some(subscriber) = session.clients.get_mut(&client_id) {
                subscriber.cols = cols;
                subscriber.rows = rows;
            }
            // `locked`: the client that set the policy is the only one that may resize.
            if session.policy == SizePolicy::Locked {
                if client_id == session.owner {
                    session.active = client_id;
                }
            } else {
                session.active = client_id;
            }
            if box_changed(session) {
                apply_layout(session_id, session, events);
            } else {
                // The request stands or was overruled either way: say which (protocol §3.2).
                broadcast_size(session_id, session, events);
            }
            None
        }
        Message::ZoomPane { id, pane_id } => {
            let mut map = sessions.lock().await;
            let Some(session_id) = session_of(&map, pane_id) else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            };
            let session = map.get_mut(&session_id)?;
            if !session.layout.zoom(pane_id) {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            }
            apply_layout(session_id, session, events);
            None
        }
        Message::Unzoom { id, pane_id } => {
            let mut map = sessions.lock().await;
            let Some(session_id) = session_of(&map, pane_id) else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            };
            let session = map.get_mut(&session_id)?;
            if !session.layout.unzoom(pane_id) {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            }
            apply_layout(session_id, session, events);
            None
        }
        Message::ClosePane { pane_id, .. } => {
            let mut map = sessions.lock().await;
            let session_id = session_of(&map, pane_id)?;
            let session = map.get_mut(&session_id)?;
            if let Some(sender) = session.panes.remove(&pane_id) {
                send_or_queue(&sender, Cmd::Kill);
            }
            let _ = session.layout.close(pane_id);
            if session.layout.is_empty() {
                // Convention: `{leaf: 0}` means "no panes left" (ids start at 1).
                let _ = events.send(Event::Layout {
                    session_id,
                    message: Message::Layout {
                        session_id,
                        tree: Node::Leaf { leaf: 0 },
                        zoom: None,
                        focus: None,
                    },
                });
                map.remove(&session_id);
            } else {
                apply_layout(session_id, session, events);
            }
            None
        }
        Message::AckStatus { pane_id, seq } => {
            if let Some(sender) = sender_of(sessions, pane_id).await {
                let _ = sender.send(Cmd::Ack(seq)).await;
            }
            None
        }
        Message::Ping { ts } => Some(Message::Pong { ts }),
        // Protocol rule 2: a message we do not know is not an error.
        other => Some(Message::Unsupported {
            t_of: other.name().to_owned(),
        }),
    }
}

/// Spawns a shell in a fresh PTY and starts its actor.
/// The palette a new pane starts with: the last `appearance` the client sent, or harp's
/// built-in dark theme before any client has spoken.
fn current_palette(slot: &PaletteSlot) -> Option<Palette> {
    slot.lock().ok().and_then(|slot| *slot)
}

fn spawn_pane(
    events: &broadcast::Sender<Event>,
    meta: &Meta,
    shell: &str,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    palette: Option<Palette>,
) -> Option<(u32, mpsc::Sender<Cmd>)> {
    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = native_pty_system().openpty(size).ok()?;
    let mut command = CommandBuilder::new(shell);
    command.env("PI_PROGRAM_STATUS", "1");
    // A terminal must tell programs what it is; without this htop/vim refuse.
    command.env(
        "TERM",
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_owned()),
    );
    command.env("COLORTERM", "truecolor");
    if let Some(dir) = cwd {
        command.cwd(dir);
    }
    let child = pair.slave.spawn_command(command).ok()?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().ok()?;
    let mut writer = pair.master.take_writer().ok()?;
    let master: Arc<StdMutex<Box<dyn MasterPty + Send>>> = Arc::new(StdMutex::new(pair.master));

    let (pty_tx, mut pty_rx) = mpsc::channel::<Vec<u8>>(64);
    let debug = std::env::var_os("PIGEON_DEBUG").is_some();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if debug {
                        eprintln!(
                            "pigeon-host: pty {:?}",
                            String::from_utf8_lossy(&buffer[..count])
                        );
                    }
                    if pty_tx.blocking_send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let (write_tx, write_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for chunk in write_rx {
            if writer.write_all(&chunk).is_err() {
                break;
            }
        }
    });

    let pane_id = NEXT_PANE.fetch_add(1, Ordering::Relaxed);
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Cmd>(64);
    let events = events.clone();
    let meta = Arc::clone(meta);
    tokio::spawn(async move {
        let mut pane = PaneState::new(pane_id, cols, rows, SCROLLBACK);
        // Panes are born with the client's palette (or the default dark one), so a program's
        // startup colour query is answered from the first byte.
        pane.set_palette(palette);
        let mut child = child;
        let mut ticker = interval(Duration::from_millis(150));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut commands_open = true;
        let mut last_revision = 0u32;
        let mut last_meta: Option<(Option<String>, Option<String>)> = None;
        // When the last delta left, and whether the screen changed since.
        let mut last_flush = Instant::now() - FRAME;
        let mut dirty = false;
        loop {
            tokio::select! {
                _ = sleep_until(last_flush + FRAME), if dirty => {
                    dirty = false;
                    last_flush = Instant::now();
                    flush(&mut pane, &events);
                }
                command = cmd_rx.recv(), if commands_open => {
                    let Some(command) = command else {
                        commands_open = false;
                        continue;
                    };
                    match command {
                        Cmd::Input(bytes) => {
                            let _ = write_tx.send(bytes);
                        }
                        Cmd::Paste(text) => {
                            let _ = write_tx.send(pane.paste_bytes(&text));
                        }
                        Cmd::Resize(cols, rows) => {
                            pane.resize(cols, rows);
                            if let Ok(master) = master.lock() {
                                let _ = master.resize(PtySize {
                                    rows,
                                    cols,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                });
                            }
                            // Everything is dirty after a resize; the snapshot each watcher
                            // gets covers it, so the damage must not go out a second time.
                            let _ = pane.take_damage();
                            let _ = events.send(Event::Resized { pane_id });
                        }
                        Cmd::Snapshot { history, reply } => {
                            let _ = reply.send(pane.snapshot_message(history));
                        }
                        Cmd::Since { since, reply } => {
                            // Pending damage first, so the replay ends at the current screen
                            // and the live stream continues right after it.
                            if dirty {
                                dirty = false;
                                last_flush = Instant::now();
                                flush(&mut pane, &events);
                            }
                            let frames = pane
                                .deltas_since(since)
                                .unwrap_or_else(|| vec![pane.snapshot_message(false)]);
                            let _ = reply.send(frames);
                        }
                        Cmd::Scroll { offset, reply } => {
                            let _ = reply.send(pane.scroll_message(offset));
                        }
                        Cmd::Kill => {
                            let _ = child.kill();
                        }
                        Cmd::Ack(seq) => {
                            pane.status_mut().ack_seq(seq);
                        }
                        Cmd::SetPalette(palette) => {
                            pane.set_palette(palette);
                            // No flush: the palette only affects query answers, not the screen.
                        }
                        Cmd::LatestStatus { reply } => {
                            let pending = pane.status().latest_unacked().is_some();
                            let _ = reply.send(pending.then(|| pane.status_message()));
                        }
                    }
                }
                _ = ticker.tick() => {
                    if let Ok(Some(status)) = child.try_wait() {
                        pane.status_mut().on_process_exit();
                        let _ = events.send(Event::Exited {
                            pane_id,
                            code: status.exit_code() as i32,
                        });
                        break;
                    }
                }
                chunk = pty_rx.recv() => {
                    let Some(chunk) = chunk else {
                        let code = child
                            .wait()
                            .map(|status| status.exit_code() as i32)
                            .unwrap_or(-1);
                        pane.status_mut().on_process_exit();
                        let _ = events.send(Event::Exited { pane_id, code });
                        break;
                    };
                    pane.feed(&chunk);
                    let cwd = pane.cwd().map(str::to_owned);
                    let title = pane.title();
                    if cwd.is_some() || title.is_some() {
                        let snapshot = (cwd.clone(), title.clone());
                        if last_meta.as_ref() != Some(&snapshot) {
                            last_meta = Some(snapshot);
                            meta.lock().await.insert(pane_id, PaneMeta { cwd, title });
                            let _ = events.send(Event::SessionsChanged);
                        }
                    }
                    let revision = pane.status().revision();
                    if revision != last_revision {
                        last_revision = revision;
                        let message = pane.status_message();
                        let _ = events.send(Event::Status { pane_id, message });
                    }
                    let replies = pane.take_replies();
                    if !replies.is_empty() {
                        let _ = write_tx.send(replies);
                    }
                    if last_flush.elapsed() >= FRAME {
                        dirty = false;
                        last_flush = Instant::now();
                        flush(&mut pane, &events);
                    } else {
                        dirty = true;
                    }
                }
            }
        }
    });

    Some((pane_id, cmd_tx))
}

fn flush(pane: &mut PaneState, events: &broadcast::Sender<Event>) {
    if let Some(message) = pane.delta_message() {
        let _ = events.send(Event::Delta {
            pane_id: pane.pane_id,
            message,
        });
    }
}

async fn send<S>(writer: &mut tokio::io::WriteHalf<S>, message: &Message) -> Result<(), String>
where
    S: AsyncWrite + Unpin,
{
    let frame = proto::encode_frame(message).map_err(|error| format!("encode: {error}"))?;
    writer
        .write_all(&frame)
        .await
        .map_err(|error| error.to_string())?;
    // tokio's stdout is buffered; without this a `--stdio` client would wait for a frame
    // that is still sitting in the writer.
    writer.flush().await.map_err(|error| error.to_string())
}

/// Bridges stdin/stdout to a running host. Frames are opaque bytes here on purpose: the
/// bridge must not care which protocol version the two ends agreed on.
///
/// With nothing listening (first connect after a reboot, or the host crashed) it starts a
/// host first, so `ssh host pigeon-host --proxy` is the only command a client ever needs.
async fn proxy(socket: &std::path::Path, shell: &str) -> Result<(), String> {
    let stream = match UnixStream::connect(socket).await {
        Ok(stream) => stream,
        Err(_) => {
            start_detached_host(socket, shell)?;
            connect_with_patience(socket).await?
        }
    };
    let mut stdio = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    let mut stream = stream;
    tokio::io::copy_bidirectional(&mut stdio, &mut stream)
        .await
        .map_err(|error| format!("proxy failed: {error}"))?;
    Ok(())
}

/// A host that must outlive this bridge: no shared stdio (the ssh channel closing would
/// break its pipes) and its own process group (sshd's hangup goes to the session's group).
fn start_detached_host(socket: &std::path::Path, shell: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let program = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    std::process::Command::new(program)
        .arg("--socket")
        .arg(socket)
        .args(["--shell", shell])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("cannot start a host: {error}"))?;
    Ok(())
}

/// The host just started needs a moment to bind its socket.
async fn connect_with_patience(socket: &std::path::Path) -> Result<UnixStream, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(socket).await {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!("cannot reach {}: {error}", socket.display()));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

fn type_tag(buf: &[u8]) -> Option<String> {
    let payload = buf.get(4..)?;
    let value: ciborium::value::Value = ciborium::from_reader(payload).ok()?;
    let ciborium::value::Value::Map(entries) = value else {
        return None;
    };
    entries
        .into_iter()
        .find_map(|(key, value)| match (key, value) {
            (ciborium::value::Value::Text(name), ciborium::value::Value::Text(tag))
                if name == "t" =>
            {
                Some(tag)
            }
            _ => None,
        })
}

/// `$XDG_RUNTIME_DIR/pigeon.sock`, else `/tmp/pigeon-<uid>.sock` (protocol §1).
///
/// shortcut: the uid comes from `id -u` because `getuid` needs `unsafe`, which
/// is banned outside `ffi.rs`; swap to a safe syscall wrapper when one is added.
fn default_socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("pigeon.sock");
    }
    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "0".to_owned());
    PathBuf::from(format!("/tmp/pigeon-{uid}.sock"))
}

/// A shell prompt or a path is not a program name: fish titles its window `~/dev`, zsh sets
/// the command, and the sidebar should read the same on both.
fn looks_like_a_prompt(title: &str) -> bool {
    let title = title.trim();
    title.is_empty()
        || title.contains('/')
        || title.contains('~')
        || title.contains('@')
        || title.contains(':')
}

fn shell_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// This process's identity for `hello_ack.instance`: pid plus start time is unique enough
/// to tell a restarted host from the one a client last saw.
fn instance() -> &'static str {
    static INSTANCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(|| {
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        format!("{:x}-{}", started, std::process::id())
    })
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "local".to_owned())
}
