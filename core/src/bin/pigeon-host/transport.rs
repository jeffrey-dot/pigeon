//! How bytes reach `handle_client`: the private unix socket, this process's stdio
//! (`--stdio`), or a bridge to another host (`--proxy`).

use std::fs::Permissions;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use tokio::net::{UnixListener, UnixStream};
use tokio::time::Instant;

use crate::cli::Transport;
use crate::client::handle_client;
use crate::host_state::HostState;
use crate::persist;

/// Serves the socket forever, plus one stdio client with `Transport::Stdio`.
pub async fn serve(socket: &Path, transport: Transport, host: HostState) -> Result<(), String> {
    let listener = bind_private(socket).await?;
    // Only the host that owns the socket may read or write the state file next to it.
    let state = persist::state_path(socket);
    let restored = persist::restore(&host, &state).await;
    if restored > 0 {
        eprintln!(
            "pigeon-host: restored {restored} session(s) from {}",
            state.display()
        );
    }
    persist::start(host.clone(), state.clone());
    eprintln!("pigeon-host listening on {}", socket.display());

    if transport == Transport::Stdio {
        // One extra client on this process's stdio. When the ssh channel closes the client
        // goes away and the host keeps serving the socket: that is what makes a remote host
        // outlive the window that started it.
        let host = host.clone();
        tokio::spawn(async move {
            let stdio = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
            if let Err(error) = handle_client(stdio, host).await {
                eprintln!("pigeon-host: stdio client gone: {error}");
            }
        });
    }

    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            () = host.stop.notified() => {
                shut_down(&host, socket, &state).await;
                return Ok(());
            }
        };
        let (stream, _) = match accepted {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("pigeon-host: accept failed: {error}");
                continue;
            }
        };
        let host = host.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, host).await {
                eprintln!("pigeon-host: client gone: {error}");
            }
        });
    }
}

/// `shutdown`: every session ends (SIGHUP → SIGKILL through `Cmd::Kill`), nothing is left
/// to restore, and the socket goes away so the next window starts a fresh host.
async fn shut_down(host: &HostState, socket: &Path, state: &Path) {
    eprintln!("pigeon-host: shutting down on request");
    let senders: Vec<_> = {
        let map = host.sessions.lock().await;
        map.values()
            .flat_map(|session| session.panes.values().cloned())
            .collect()
    };
    for sender in senders {
        let _ = sender.send(crate::cmd::Cmd::Kill).await;
    }
    host.sessions.lock().await.clear();
    let _ = std::fs::remove_file(socket);
    // Give the pane tasks a moment to deliver the kills before the runtime goes away. The
    // state file goes last, so a save already in flight cannot write it back.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = std::fs::remove_file(state);
}

/// Takes over `socket` only if nobody answers on it: a live host keeps its socket and its
/// sessions, and this process exits instead (a stray start, a `--stdio` racing one).
///
/// The socket must never be reachable by other users, not even between `bind` and `chmod`.
/// Changing the umask needs `unsafe`, so the socket is bound inside a fresh 0700 directory
/// (unreachable for anyone else), chmodded there, and only then renamed into place.
async fn bind_private(socket: &Path) -> Result<UnixListener, String> {
    if std::fs::metadata(socket).is_ok_and(|meta| meta.file_type().is_socket()) {
        if UnixStream::connect(socket).await.is_ok() {
            return Err(format!("another pigeon-host is already serving {socket:?}"));
        }
        // Nobody listening: a crashed host left its socket behind.
        let _ = std::fs::remove_file(socket);
    }
    let parent = socket.parent().unwrap_or_else(|| Path::new("."));
    // `~/.pigeon/state` on a fresh install: create it private, never loosen one that exists.
    if !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|error| format!("mkdir {parent:?}: {error}"))?;
        std::fs::set_permissions(parent, Permissions::from_mode(0o700))
            .map_err(|error| format!("chmod {parent:?}: {error}"))?;
    }
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

/// Bridges stdin/stdout to a running host. Frames are opaque bytes here on purpose: the
/// bridge must not care which protocol version the two ends agreed on.
///
/// With nothing listening (first connect after a reboot, or the host crashed) it starts a
/// host first, so `ssh host pigeon-host --proxy` is the only command a client ever needs.
pub async fn proxy(socket: &Path, shell: &str) -> Result<(), String> {
    let stream = match UnixStream::connect(socket).await {
        Ok(stream) => stream,
        Err(_) => {
            start_detached_host(socket, shell, None)?;
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

/// `--detach`: makes sure a host serves `socket`, then returns. A live host is left alone;
/// otherwise one is started in its own process group and this waits until it answers, so
/// the caller can connect as soon as this exits. Nothing is registered with the system
/// (no launchd job): like a `tmux` server, the host lives until it is stopped or the
/// user logs out.
pub async fn detach(socket: &Path, shell: &str, log: Option<&Path>) -> Result<(), String> {
    if UnixStream::connect(socket).await.is_ok() {
        return Ok(());
    }
    start_detached_host(socket, shell, log)?;
    connect_with_patience(socket).await.map(drop)
}

/// A host that must outlive this bridge: no shared stdio (the ssh channel closing would
/// break its pipes) and its own process group (sshd's hangup goes to the session's group).
fn start_detached_host(socket: &Path, shell: &str, log: Option<&Path>) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let program = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    // The log is appended to, so a restarted host does not erase why the last one died.
    let output = match log {
        Some(path) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|error| format!("open {path:?}: {error}"))?;
            let copy = file.try_clone().map_err(|error| format!("log: {error}"))?;
            (
                std::process::Stdio::from(file),
                std::process::Stdio::from(copy),
            )
        }
        None => (std::process::Stdio::null(), std::process::Stdio::null()),
    };
    std::process::Command::new(program)
        .arg("--socket")
        .arg(socket)
        .args(["--shell", shell])
        .stdin(std::process::Stdio::null())
        .stdout(output.0)
        .stderr(output.1)
        .process_group(0)
        .spawn()
        .map_err(|error| format!("cannot start a host: {error}"))?;
    Ok(())
}

/// The host just started needs a moment to bind its socket.
async fn connect_with_patience(socket: &Path) -> Result<UnixStream, String> {
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
