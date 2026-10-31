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

/// Serves the socket forever, plus one stdio client with `Transport::Stdio`.
pub async fn serve(socket: &Path, transport: Transport, host: HostState) -> Result<(), String> {
    let listener = bind_private(socket).await?;
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
        let (stream, _) = match listener.accept().await {
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
fn start_detached_host(socket: &Path, shell: &str) -> Result<(), String> {
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
