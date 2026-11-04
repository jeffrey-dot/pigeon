//! The connection to `pigeon-server`: where the socket is, how to start a host, and the two
//! threads that carry frames.
//!
//! The client never computes the socket path itself — it asks the server for it
//! (`pigeon-server --socket-path`, protocol §1), so the rule lives in one place.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use pigeon_core::api::{decode_frame, encode_frame, Message, PaneId, PROTO_VERSION};

use crate::screen::Screen;

const CLIENT_NAME: &str = concat!("pigeon-linux ", env!("CARGO_PKG_VERSION"));

/// The two directions of one connection. Both halves clone cheaply, so the UI thread can
/// send while the pump holds the receiver.
#[derive(Clone)]
pub struct HostLink {
    tx: Sender<Message>,
    rx: Receiver<Message>,
}

impl HostLink {
    /// Connects to the local host, starting one first if nothing is listening.
    pub fn connect() -> Result<Self, String> {
        let server = server_binary()?;
        let socket = socket_path(&server)?;
        ensure_host(&server, &socket)?;
        let stream =
            UnixStream::connect(&socket).map_err(|error| format!("{socket:?}: {error}"))?;
        Ok(Self::spawn_threads(stream))
    }

    pub fn send(&self, message: Message) {
        // The writer thread owns the socket; a dead thread means the reader has reported the
        // disconnect already, and the pump will stop on its own.
        let _ = self.tx.send_blocking(message);
    }

    /// Blocking receive, used only during the handshake (before the GTK loop runs).
    pub fn recv(&self) -> Option<Message> {
        self.rx.recv_blocking().ok()
    }

    pub fn receiver(&self) -> Receiver<Message> {
        self.rx.clone()
    }

    fn spawn_threads(stream: UnixStream) -> Self {
        let (tx, writer_rx) = async_channel::unbounded::<Message>();
        let (reader_tx, rx) = async_channel::unbounded::<Message>();

        let mut writer = match stream.try_clone() {
            Ok(writer) => writer,
            Err(error) => {
                eprintln!("pigeon: socket clone failed: {error}");
                return Self { tx, rx };
            }
        };
        thread::spawn(move || {
            while let Ok(message) = writer_rx.recv_blocking() {
                match encode_frame(&message) {
                    Ok(frame) => {
                        if writer.write_all(&frame).is_err() {
                            break;
                        }
                    }
                    Err(error) => eprintln!("pigeon: cannot encode {}: {error}", message.name()),
                }
            }
            let _ = writer.flush();
        });

        let mut reader = stream;
        thread::spawn(move || {
            let mut buffer = Vec::with_capacity(64 * 1024);
            let mut chunk = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        buffer.extend_from_slice(&chunk[..read]);
                        loop {
                            match decode_frame(&buffer) {
                                Ok(Some((message, used))) => {
                                    buffer.drain(..used);
                                    if reader_tx.send_blocking(message).is_err() {
                                        return;
                                    }
                                }
                                Ok(None) => break,
                                Err(error) => {
                                    eprintln!("pigeon: bad frame: {error}");
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("pigeon: socket read: {error}");
                        break;
                    }
                }
            }
        });

        Self { tx, rx }
    }
}

/// `hello` → (`sessions` or a new session) → `attach`, ending with the pane's first screen.
///
/// Runs before the GTK loop starts, so a failure can be reported on the terminal instead of
/// leaving an empty window behind.
pub fn handshake(link: &HostLink) -> Result<Screen, String> {
    link.send(Message::Hello {
        proto: PROTO_VERSION,
        client: CLIENT_NAME.to_owned(),
        // Both snapshots and deltas use the core's safe cell conversion.
        features: vec!["delta".to_owned()],
    });
    loop {
        match link.recv().ok_or("host closed during the handshake")? {
            Message::HelloAck { proto, server, .. } => {
                if proto >> 8 != PROTO_VERSION >> 8 {
                    return Err(format!("host speaks {proto:#06x}: {server}"));
                }
                break;
            }
            Message::HelloError { reason, detail, .. } => {
                return Err(format!("host refused the handshake: {reason} ({detail})"))
            }
            Message::Error { code, detail, .. } => return Err(format!("{code}: {detail}")),
            _ => {}
        }
    }

    link.send(Message::Sessions { items: Vec::new() });
    let existing = loop {
        match link.recv().ok_or("host closed before sending sessions")? {
            Message::Sessions { items } => {
                break items.first().and_then(|session| {
                    session
                        .active_pane
                        .or_else(|| session.panes.first().copied())
                });
            }
            Message::Error { code, detail, .. } => return Err(format!("{code}: {detail}")),
            _ => {}
        }
    };

    let pane_id = match existing {
        Some(pane_id) => pane_id,
        None => new_session(link)?,
    };
    attach(link, pane_id)
}

fn new_session(link: &HostLink) -> Result<PaneId, String> {
    link.send(Message::NewSession {
        id: 1,
        cwd: None,
        shell: None,
        env: None,
    });
    loop {
        match link.recv().ok_or("host closed before creating a session")? {
            Message::Created { pane_id, .. } => return Ok(pane_id),
            Message::Error { code, detail, .. } => return Err(format!("{code}: {detail}")),
            _ => {}
        }
    }
}

fn attach(link: &HostLink, pane_id: PaneId) -> Result<Screen, String> {
    link.send(Message::Attach {
        id: 2,
        pane_id,
        size_policy: None,
        // The scope has no scrollback UI yet; asking for it would move up to 2 MB per attach.
        history: Some(false),
        since: None,
    });
    loop {
        let message = link.recv().ok_or("host closed before the first snapshot")?;
        if let Message::Error { code, detail, .. } = &message {
            return Err(format!("{code}: {detail}"));
        }
        if let Some(screen) = Screen::from_message(&message) {
            return Ok(screen);
        }
    }
}

fn server_binary() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PIGEON_SERVER") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = search_path("pigeon-server") {
        return Ok(path);
    }
    // Dev convenience: the sibling build of `core/` (AGENTS.md §2).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/target");
    for profile in ["debug", "release"] {
        let candidate = root.join(profile).join("pigeon-server");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("pigeon-server not found: run `cargo build --manifest-path core/Cargo.toml` or set PIGEON_SERVER"
        .to_owned())
}

fn search_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn socket_path(server: &Path) -> Result<PathBuf, String> {
    let output = Command::new(server)
        .arg("--socket-path")
        .output()
        .map_err(|error| format!("{}: {error}", server.display()))?;
    if !output.status.success() {
        return Err(format!("{} --socket-path failed", server.display()));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "socket path is not UTF-8")?;
    Ok(PathBuf::from(text.trim()))
}

/// Starts a detached host when nothing answers on the socket; `--detach` does the waiting,
/// so a second client does not race the first one into spawning two hosts.
fn ensure_host(server: &Path, socket: &Path) -> Result<(), String> {
    if UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    let log = log_path();
    let _ = Command::new(server)
        .arg("--detach")
        .arg("--socket")
        .arg(socket)
        .arg("--log")
        .arg(&log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "no host on {socket:?} and none started; see {log:?}"
    ))
}

/// Same directory the rest of the client uses for state (`~/.config/pigeon`), so the log is
/// next to the config rather than in `/tmp` where it would be cleaned up.
fn log_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("pigeon").join("pigeon-server.log")
}
