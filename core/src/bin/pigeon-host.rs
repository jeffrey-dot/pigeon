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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time::{interval, MissedTickBehavior};

use pigeon_core::error::PgError;
use pigeon_core::layout::{Axis, Layout, LayoutNode};
use pigeon_core::proto::{self, Message, Node, SplitAxis, SplitNode, PROTO_VERSION};
use pigeon_core::session::PaneState;

/// Capabilities this build offers (protocol §5).
const SERVER_FEATURES: &[&str] = &["delta", "scrollback", "osc7501"];
const SCROLLBACK: usize = 1000;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Pane ids are unique for the whole process, not per session.
static NEXT_PANE: AtomicU32 = AtomicU32::new(1);

/// Metadata a pane reports about itself (program-driven, not guessed).
#[derive(Clone, Default)]
struct PaneMeta {
    cwd: Option<String>,
    title: Option<String>,
}

/// One session: one layout tree plus the panes its leaves point at.
struct Session {
    layout: Layout,
    panes: HashMap<u32, mpsc::Sender<Cmd>>,
    /// Shell used for spawns; its basename is the fallback pane label.
    shell: String,
    /// cwd the session was created with (OSC 7 overrides it per pane).
    cwd: String,
    /// The session box in cells, derived from whichever pane last reported.
    cols: u16,
    rows: u16,
}

type Meta = Arc<Mutex<HashMap<u32, PaneMeta>>>;

type Sessions = Arc<Mutex<HashMap<u32, Session>>>;

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
    Status {
        pane_id: u32,
        message: Message,
    },
    /// Titles/cwd changed: clients should refresh `sessions`.
    SessionsChanged,
}

enum Cmd {
    Input(Vec<u8>),
    Resize(u16, u16),
    /// The user has seen this status record.
    Ack(u32),
    /// Terminate the child (protocol `close_pane`).
    Kill,
    Snapshot {
        history: bool,
        reply: oneshot::Sender<Message>,
    },
    LatestStatus {
        reply: oneshot::Sender<Option<Message>>,
    },
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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = PathBuf::from(args.next().ok_or("--socket needs a path")?),
            "--shell" => shell = args.next().ok_or("--shell needs a path")?,
            "-h" | "--help" => {
                println!("usage: pigeon-host [--socket PATH] [--shell PATH]");
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let (events, _) = broadcast::channel::<Event>(256);
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let meta: Meta = Arc::new(Mutex::new(HashMap::new()));

    if std::fs::metadata(&socket).is_ok_and(|meta| meta.file_type().is_socket()) {
        let _ = std::fs::remove_file(&socket);
    }
    let listener =
        UnixListener::bind(&socket).map_err(|error| format!("bind {socket:?}: {error}"))?;
    std::fs::set_permissions(&socket, Permissions::from_mode(0o700))
        .map_err(|error| format!("chmod {socket:?}: {error}"))?;
    eprintln!("pigeon-host listening on {}", socket.display());

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
        let shell = shell.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, sessions, events, meta, shell).await {
                eprintln!("pigeon-host: client gone: {error}");
            }
        });
    }
}

#[derive(Default)]
struct ClientState {
    greeted: bool,
    /// Panes this connection gets live output for → negotiated `delta`.
    watched: HashMap<u32, bool>,
    /// The session whose layout this connection follows (one per window in M1).
    session: Option<u32>,
    /// Whether the handshake negotiated `delta` for this connection.
    delta_ok: bool,
}

async fn handle_client(
    stream: UnixStream,
    sessions: Sessions,
    events: broadcast::Sender<Event>,
    meta: Meta,
    shell: String,
) -> Result<(), String> {
    let (mut reader, mut writer) = stream.into_split();
    let mut inbox = Vec::new();
    let mut subscribed = events.subscribe();
    let mut state = ClientState::default();

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
                            if let Some(reply) = dispatch(message, &sessions, &events, &meta, &shell, &mut state).await {
                                let handshake = matches!(reply, Message::HelloAck { .. });
                                let attached_pane = match &reply {
                                    Message::Snapshot { pane_id, .. } => Some(*pane_id),
                                    _ => None,
                                };
                                send(&mut writer, &reply).await?;
                                if let Some(pane_id) = attached_pane {
                                    if let Some(status) = latest_status(&sessions, pane_id).await {
                                        send(&mut writer, &status).await?;
                                    }
                                }
                                // Protocol §3.2: the session list follows the ack.
                                if handshake {
                                    if let Some(list) = dispatch(
                                        Message::Sessions { items: Vec::new() },
                                        &sessions,
                                        &events,
                                        &meta,
                                        &shell,
                                        &mut state,
                                    )
                                    .await
                                    {
                                        send(&mut writer, &list).await?;
                                    }
                                }
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
                            &sessions,
                            &events,
                            &meta,
                            &shell,
                            &mut state,
                        )
                        .await
                        {
                            send(&mut writer, &list).await?;
                        }
                    }
                    Event::Delta { pane_id, message } => {
                        let Some(delta_ok) = state.watched.get(&pane_id).copied() else {
                            continue;
                        };
                        if delta_ok {
                            send(&mut writer, &message).await?;
                        } else if let Some(snapshot) = snapshot_of(&sessions, pane_id, false).await {
                            send(&mut writer, &snapshot).await?;
                        }
                    }
                }
            }
        }
    }
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
        zoom: None,
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

/// Re-fits every pane to its rect and tells clients what the tree looks like.
fn apply_layout(session_id: u32, session: &mut Session, events: &broadcast::Sender<Event>) {
    for (pane_id, rect) in session.layout.rects() {
        let cols = ((rect.w * f32::from(session.cols)).round() as u16).max(2);
        let rows = ((rect.h * f32::from(session.rows)).round() as u16).max(1);
        if let Some(sender) = session.panes.get(&pane_id) {
            // try_send: never block while holding the sessions lock.
            let _ = sender.try_send(Cmd::Resize(cols, rows));
        }
    }
    let _ = events.send(Event::Layout {
        session_id,
        message: layout_message(session_id, session),
    });
}

async fn dispatch(
    message: Message,
    sessions: &Sessions,
    events: &broadcast::Sender<Event>,
    meta: &Meta,
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
                client_id: 1,
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
                        proto::SessionInfo {
                            session_id: *session_id,
                            title: format!("session {session_id}"),
                            cwd: info
                                .and_then(|info| info.cwd.clone())
                                .unwrap_or_else(|| session.cwd.clone()),
                            panes: session.layout.pane_ids(),
                            active_pane: active,
                            active_title: Some(
                                info.and_then(|info| info.title.clone())
                                    .unwrap_or_else(|| session.shell.clone()),
                            ),
                        }
                    })
                    .collect(),
            })
        }
        Message::NewSession {
            id,
            cwd,
            shell: requested,
            ..
        } => {
            let shell = requested.unwrap_or_else(|| shell.to_owned());
            let launch_cwd = cwd.clone().unwrap_or_default();
            let Some((pane_id, sender)) =
                spawn_pane(events, meta, &shell, cwd, DEFAULT_COLS, DEFAULT_ROWS)
            else {
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
            };
            sessions.lock().await.insert(pane_id, session);
            Some(Message::Created {
                id,
                session_id: pane_id,
                pane_id,
            })
        }
        Message::Attach { id, pane_id, .. } => {
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
            let Some(snapshot) = snapshot_of(sessions, pane_id, true).await else {
                return Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                });
            };
            state.watched.insert(pane_id, state.delta_ok);
            state.session = Some(session_id);
            // The tree follows the snapshot, so a reconnecting window redraws
            // the same split geometry.
            {
                let map = sessions.lock().await;
                if let Some(session) = map.get(&session_id) {
                    let _ = events.send(Event::Layout {
                        session_id,
                        message: layout_message(session_id, session),
                    });
                }
            }
            Some(snapshot)
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
            let Some((new_pane, sender)) = spawn_pane(events, meta, shell, None, cols, rows) else {
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
            apply_layout(session_id, session, events);
            None
        }
        Message::Keys { pane_id, bytes } => {
            if let Some(sender) = sender_of(sessions, pane_id).await {
                let _ = sender.send(Cmd::Input(bytes)).await;
            }
            None
        }
        Message::ResizePane {
            pane_id,
            cols,
            rows,
        } => {
            let mut map = sessions.lock().await;
            if let Some(session_id) = session_of(&map, pane_id) {
                if let Some(session) = map.get_mut(&session_id) {
                    // Derive the session box from the pane's own rect.
                    if let Some((_, rect)) = session
                        .layout
                        .rects()
                        .into_iter()
                        .find(|(id, _)| *id == pane_id)
                    {
                        if rect.w > 0.0 {
                            session.cols = ((f32::from(cols) / rect.w).round() as u16).max(2);
                        }
                        if rect.h > 0.0 {
                            session.rows = ((f32::from(rows) / rect.h).round() as u16).max(1);
                        }
                    }
                }
                if let Some(sender) = map
                    .get(&session_id)
                    .and_then(|session| session.panes.get(&pane_id))
                    .cloned()
                {
                    drop(map);
                    let _ = sender.send(Cmd::Resize(cols, rows)).await;
                }
            }
            None
        }
        Message::ClosePane { pane_id, .. } => {
            let mut map = sessions.lock().await;
            let session_id = session_of(&map, pane_id)?;
            let session = map.get_mut(&session_id)?;
            if let Some(sender) = session.panes.remove(&pane_id) {
                let _ = sender.try_send(Cmd::Kill);
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
        // `zoom_pane` / `unzoom` are not implemented in M1 (protocol rule 2).
        other => Some(Message::Unsupported {
            t_of: other.name().to_owned(),
        }),
    }
}

/// Spawns a shell in a fresh PTY and starts its actor.
fn spawn_pane(
    events: &broadcast::Sender<Event>,
    meta: &Meta,
    shell: &str,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
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
        let mut child = child;
        let mut ticker = interval(Duration::from_millis(150));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut commands_open = true;
        let mut last_revision = 0u32;
        let mut last_meta: Option<(Option<String>, Option<String>)> = None;
        loop {
            tokio::select! {
                command = cmd_rx.recv(), if commands_open => {
                    let Some(command) = command else {
                        commands_open = false;
                        continue;
                    };
                    match command {
                        Cmd::Input(bytes) => {
                            let _ = write_tx.send(bytes);
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
                            flush(&mut pane, &events);
                        }
                        Cmd::Snapshot { history, reply } => {
                            let _ = reply.send(pane.snapshot_message(history));
                        }
                        Cmd::Kill => {
                            let _ = child.kill();
                        }
                        Cmd::Ack(seq) => {
                            pane.status_mut().ack_seq(seq);
                        }
                        Cmd::LatestStatus { reply } => {
                            let _ = reply.send(pane.status().latest_unacked().map(|record| Message::Status {
                                pane_id,
                                state: record.state.as_str().to_owned(),
                                seq: record.seq,
                                kind: (record.kind.as_str() != "").then(|| record.kind.as_str().to_owned()),
                                msg: (!record.msg.is_empty()).then(|| record.msg.clone()),
                            }));
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
                        // A cleared record set must reach clients too: "idle" with
                        // seq 0 means "nothing is pending".
                        let message = match pane.status().latest_unacked() {
                            Some(record) => Message::Status {
                                pane_id,
                                state: record.state.as_str().to_owned(),
                                seq: record.seq,
                                kind: (record.kind.as_str() != "").then(|| record.kind.as_str().to_owned()),
                                msg: (!record.msg.is_empty()).then(|| record.msg.clone()),
                            },
                            None => Message::Status {
                                pane_id,
                                state: "idle".to_owned(),
                                seq: 0,
                                kind: None,
                                msg: None,
                            },
                        };
                        let _ = events.send(Event::Status { pane_id, message });
                    }
                    let replies = pane.take_replies();
                    if !replies.is_empty() {
                        let _ = write_tx.send(replies);
                    }
                    flush(&mut pane, &events);
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

async fn send(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &Message,
) -> Result<(), String> {
    let frame = proto::encode_frame(message).map_err(|error| format!("encode: {error}"))?;
    writer
        .write_all(&frame)
        .await
        .map_err(|error| error.to_string())
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

fn shell_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "local".to_owned())
}
