//! Session host: owns the PTYs and outlives every client window.
//!
//! One tokio runtime, one task per pane (that task is the only owner of the
//! pane's terminal state), one task per client. A client disconnecting only drops
//! its task — the PTY and the pane state keep running, which is the point of
//! pigeon.

use std::collections::HashMap;
use std::fs::Permissions;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time::{interval, MissedTickBehavior};

use pigeon_core::error::PgError;
use pigeon_core::proto::{self, Message, PROTO_VERSION};
use pigeon_core::session::PaneState;

/// Capabilities this build offers (protocol §5).
const SERVER_FEATURES: &[&str] = &["delta", "scrollback", "osc7501"];
const SCROLLBACK: usize = 1000;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

type Sessions = Arc<Mutex<HashMap<u32, Handle>>>;

#[derive(Clone)]
enum Event {
    Delta { pane_id: u32, message: Message },
    Exited { pane_id: u32, code: i32 },
}

impl Event {
    fn pane_id(&self) -> u32 {
        match self {
            Self::Delta { pane_id, .. } | Self::Exited { pane_id, .. } => *pane_id,
        }
    }
}

enum Cmd {
    Input(Vec<u8>),
    Resize(u16, u16),
    /// Terminate the child (protocol `close_pane`).
    Kill,
    Snapshot {
        history: bool,
        reply: oneshot::Sender<Message>,
    },
}

struct Handle {
    cmd: mpsc::Sender<Cmd>,
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
        let shell = shell.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, sessions, events, shell).await {
                eprintln!("pigeon-host: client gone: {error}");
            }
        });
    }
}

#[derive(Default)]
struct ClientState {
    greeted: bool,
    attached: Option<(u32, bool)>,
}

async fn handle_client(
    stream: UnixStream,
    sessions: Sessions,
    events: broadcast::Sender<Event>,
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
                            if let Some(reply) = dispatch(message, &sessions, &events, &shell, &mut state).await {
                                let handshake = matches!(reply, Message::HelloAck { .. });
                                send(&mut writer, &reply).await?;
                                // Protocol §3.2: the session list follows the ack.
                                if handshake {
                                    if let Some(list) = dispatch(
                                        Message::Sessions { items: Vec::new() },
                                        &sessions,
                                        &events,
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
                if state.attached.map(|(pane, _)| pane) != Some(event.pane_id()) {
                    continue;
                }
                match event {
                    Event::Exited { pane_id, code } => {
                        send(&mut writer, &Message::PaneExited { pane_id, code }).await?;
                    }
                    Event::Delta { pane_id, message } => {
                        let delta_ok = state.attached.map(|(_, delta)| delta).unwrap_or(false);
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
    let (tx, rx) = oneshot::channel();
    {
        let handles = sessions.lock().await;
        handles
            .get(&pane_id)?
            .cmd
            .send(Cmd::Snapshot { history, reply: tx })
            .await
            .ok()?;
    }
    rx.await.ok()
}

async fn dispatch(
    message: Message,
    sessions: &Sessions,
    events: &broadcast::Sender<Event>,
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
            state.attached = Some((0, offered.iter().any(|f| f == "delta")));
            Some(Message::HelloAck {
                proto: PROTO_VERSION,
                server: format!("pigeon-host {}", env!("CARGO_PKG_VERSION")),
                features: offered,
                host_id: hostname(),
                client_id: 1,
            })
        }
        Message::Sessions { .. } => Some(Message::Sessions {
            items: sessions
                .lock()
                .await
                .keys()
                .map(|pane_id| proto::SessionInfo {
                    session_id: *pane_id,
                    title: format!("pane {pane_id}"),
                    cwd: String::new(),
                    panes: vec![*pane_id],
                })
                .collect(),
        }),
        Message::NewSession {
            id,
            cwd,
            shell: requested,
            ..
        } => {
            let shell = requested.unwrap_or_else(|| shell.to_owned());
            match spawn_session(sessions, events, &shell, cwd).await {
                Some(pane_id) => Some(Message::Created {
                    id,
                    session_id: pane_id,
                    pane_id,
                }),
                None => Some(Message::Error {
                    code: "internal".into(),
                    detail: "cannot spawn the shell".into(),
                    id: Some(id),
                }),
            }
        }
        Message::Attach { id, pane_id, .. } => {
            let delta_ok = state.attached.map(|(_, delta)| delta).unwrap_or(false);
            match snapshot_of(sessions, pane_id, true).await {
                Some(snapshot) => {
                    state.attached = Some((pane_id, delta_ok));
                    Some(snapshot)
                }
                None => Some(Message::Error {
                    code: "no_such_pane".into(),
                    detail: format!("pane {pane_id}"),
                    id: Some(id),
                }),
            }
        }
        Message::Keys { pane_id, bytes } => {
            forward(sessions, pane_id, Cmd::Input(bytes)).await;
            None
        }
        Message::ResizePane {
            pane_id,
            cols,
            rows,
        } => {
            forward(sessions, pane_id, Cmd::Resize(cols, rows)).await;
            None
        }
        Message::ClosePane { pane_id, .. } => {
            // Close means close: terminate the child and drop the pane.
            forward(sessions, pane_id, Cmd::Kill).await;
            sessions.lock().await.remove(&pane_id);
            None
        }
        Message::Ping { ts } => Some(Message::Pong { ts }),
        // Client-to-server messages outside the M0 subset.
        other => Some(Message::Unsupported {
            t_of: other.name().to_owned(),
        }),
    }
}

async fn forward(sessions: &Sessions, pane_id: u32, cmd: Cmd) {
    let handles = sessions.lock().await;
    if let Some(handle) = handles.get(&pane_id) {
        let _ = handle.cmd.send(cmd).await;
    }
}

async fn spawn_session(
    sessions: &Sessions,
    events: &broadcast::Sender<Event>,
    shell: &str,
    cwd: Option<String>,
) -> Option<u32> {
    let size = PtySize {
        rows: DEFAULT_ROWS,
        cols: DEFAULT_COLS,
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

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Cmd>(64);
    let pane_id = {
        let mut handles = sessions.lock().await;
        let pane_id = handles.keys().max().copied().unwrap_or(0) + 1;
        handles.insert(pane_id, Handle { cmd: cmd_tx });
        pane_id
    };

    let events = events.clone();
    tokio::spawn(async move {
        let mut pane = PaneState::new(pane_id, DEFAULT_COLS, DEFAULT_ROWS, SCROLLBACK);
        let mut child = child;
        let mut ticker = interval(Duration::from_millis(150));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut commands_open = true;
        loop {
            tokio::select! {
                command = cmd_rx.recv(), if commands_open => {
                    let Some(command) = command else {
                        // The session was dropped: stop taking commands but keep
                        // running until the child is reaped, so `pane_exited`
                        // still reaches the remaining clients.
                        commands_open = false;
                        continue;
                    };
                    match command {
                        Cmd::Input(bytes) => {
                            // Keystrokes go to the child; the echo comes back
                            // through the PTY reader and is fed there.
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
                        // PTY gone: report the real exit code, then stop.
                        let code = child
                            .wait()
                            .map(|status| status.exit_code() as i32)
                            .unwrap_or(-1);
                        pane.status_mut().on_process_exit();
                        let _ = events.send(Event::Exited { pane_id, code });
                        break;
                    };
                    pane.feed(&chunk);
                    let replies = pane.take_replies();
                    if !replies.is_empty() {
                        let _ = write_tx.send(replies);
                    }
                    flush(&mut pane, &events);
                }
            }
        }
    });

    Some(pane_id)
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

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "local".to_owned())
}
