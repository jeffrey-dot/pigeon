//! The connection to `pigeon-server`: socket discovery, host startup, frame pumps and idle
//! keepalive.
//!
//! Ported from the Linux client (`clients/linux/src/host.rs`): the client never computes the
//! socket path itself — it asks the server for it (`pigeon-server --socket-path`, protocol
//! §1), so the rule lives in one place. Reconnect is not wired yet, so this copy keeps only
//! connect/handshake/send/receive.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use pigeon_core::api::{Message, PROTO_VERSION};
mod transport;
use transport::Transport;

const CLIENT_NAME: &str = concat!("pigeon-gpui ", env!("CARGO_PKG_VERSION"));

/// The two directions of one connection. Both halves clone cheaply, so the UI thread can
/// send while the pump holds the receiver.
#[derive(Clone)]
pub struct HostLink {
    transport: Transport,
    scrollback: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl HostLink {
    /// Connects to the local host, starting one first if nothing is listening.
    pub fn connect() -> Result<Self, String> {
        let server = server_binary()?;
        let socket = socket_path(&server)?;
        ensure_host(&server, &socket)?;
        let stream =
            UnixStream::connect(&socket).map_err(|error| format!("{socket:?}: {error}"))?;
        Ok(Self {
            transport: Transport::new(stream)?,
            scrollback: Default::default(),
        })
    }

    pub fn send(&self, message: Message) {
        self.transport.send(message);
    }
    pub fn recv(&self) -> Option<Message> {
        self.transport.recv()
    }
    pub async fn next(&self) -> Option<Message> {
        self.transport.next().await
    }
    pub fn reason(&self) -> String {
        self.transport.reason()
    }
}

/// Only consumes hello_ack. Sessions, status and screens stay in order for the UI reducer.
pub fn handshake(link: &HostLink) -> Result<Message, String> {
    link.send(Message::Hello {
        proto: PROTO_VERSION,
        client: CLIENT_NAME.to_owned(),
        features: [
            "delta",
            "scrollback",
            "status_all",
            "status_snapshot",
            "text_query",
            "text_viewport",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    });
    match link.recv().ok_or_else(|| link.reason())? {
        message @ Message::HelloAck { .. } => {
            if let Message::HelloAck {
                proto, features, ..
            } = &message
            {
                if proto >> 8 != PROTO_VERSION >> 8 {
                    return Err(format!("host speaks {proto:#06x}"));
                }
                link.scrollback.store(
                    features.iter().any(|f| f == "scrollback"),
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
            link.transport.ready();
            Ok(message)
        }
        Message::HelloError { reason, detail } => Err(format!("host refused: {reason} ({detail})")),
        other => Err(format!("expected hello_ack, got {}", other.name())),
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
