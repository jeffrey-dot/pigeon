//! Who may own the socket and the directory it lives in. Another local user must not be
//! able to put a socket of their own at our path (and read every keystroke) or swap ours
//! out. The rules, checked with `lstat` so a symlink is never followed:
//!
//! - the directory is a real directory, and either ours and not writable by group/other
//!   (`$XDG_RUNTIME_DIR`, `~/.pigeon/state`, `/tmp/pigeon-<uid>/`), or root's with the
//!   sticky bit (`/tmp` itself: others cannot remove or rename what we put there);
//! - a socket already at the path is a socket and ours;
//! - the process on the other end of a connection runs as us (`peer_cred`).
//!
//! The macOS client applies the same rules (`Net/SocketTrust.swift`).

use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

/// The uid this process runs as.
pub fn current_uid() -> u32 {
    nix::unistd::getuid().as_raw()
}

const STICKY: u32 = 0o1000;

/// Whether `dir` may hold our socket. The directory is resolved first (macOS `/tmp` is a
/// link to `/private/tmp`); a link to someone else's directory then fails the owner check.
pub fn check_dir(dir: &Path, uid: u32) -> Result<(), String> {
    let resolved =
        std::fs::canonicalize(dir).map_err(|error| format!("resolve {dir:?}: {error}"))?;
    let meta = std::fs::symlink_metadata(&resolved)
        .map_err(|error| format!("lstat {resolved:?}: {error}"))?;
    if !meta.file_type().is_dir() {
        return Err(format!("{dir:?} is not a directory"));
    }
    let mode = meta.mode();
    let ours = meta.uid() == uid && mode & 0o022 == 0;
    let shared_sticky = meta.uid() == 0 && mode & STICKY != 0;
    if ours || shared_sticky {
        Ok(())
    } else {
        Err(format!(
            "{dir:?} is owned by uid {} with mode {:o}; it must be ours and not writable by others",
            meta.uid(),
            mode & 0o7777
        ))
    }
}

/// Whether what sits at `socket` is ours: `Ok(false)` when nothing is there.
pub fn check_existing_socket(socket: &Path, uid: u32) -> Result<bool, String> {
    let meta = match std::fs::symlink_metadata(socket) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("lstat {socket:?}: {error}")),
    };
    if !meta.file_type().is_socket() {
        return Err(format!("{socket:?} exists and is not a socket"));
    }
    if meta.uid() != uid {
        return Err(format!("{socket:?} belongs to uid {}", meta.uid()));
    }
    Ok(true)
}

/// Both checks for a socket we are about to connect to.
pub fn check_socket_path(socket: &Path, uid: u32) -> Result<(), String> {
    check_dir(socket.parent().unwrap_or_else(|| Path::new(".")), uid)?;
    match check_existing_socket(socket, uid)? {
        true => Ok(()),
        false => Err(format!("{socket:?} does not exist")),
    }
}

/// The other end of `stream` runs as `uid`.
pub fn check_peer(stream: &tokio::net::UnixStream, uid: u32) -> Result<(), String> {
    let peer = stream
        .peer_cred()
        .map_err(|error| format!("peer credentials: {error}"))?;
    if peer.uid() == uid {
        Ok(())
    } else {
        Err(format!("peer runs as uid {}, not {uid}", peer.uid()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    /// Under `/tmp`, not `$TMPDIR`: macOS's per-user temp path is too long for `sun_path`.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::path::Path::new("/tmp").join(format!(
            "pigeon-trust-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn a_private_dir_is_trusted_and_a_writable_one_is_not() {
        let uid = current_uid();
        let dir = scratch("dir");
        std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).expect("chmod");
        assert!(check_dir(&dir, uid).is_ok());
        std::fs::set_permissions(&dir, Permissions::from_mode(0o777)).expect("chmod");
        assert!(
            check_dir(&dir, uid).is_err(),
            "anyone could swap the socket"
        );
        std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).expect("chmod");
        // Someone else's uid owning the directory.
        assert!(check_dir(&dir, uid.wrapping_add(1)).is_err());
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_symlinked_socket_is_refused() {
        let uid = current_uid();
        let dir = scratch("link");
        std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).expect("chmod");
        let fake = dir.join("pigeon.sock");
        std::fs::write(dir.join("file"), b"").expect("write");
        std::os::unix::fs::symlink(dir.join("file"), &fake).expect("symlink");
        assert!(check_existing_socket(&fake, uid).is_err());
        let _ = std::fs::remove_file(&fake);
        let _ = std::fs::remove_file(dir.join("file"));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn our_socket_passes_and_a_missing_one_is_reported() {
        let uid = current_uid();
        let dir = scratch("sock");
        std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).expect("chmod");
        let socket = dir.join("pigeon.sock");
        assert_eq!(check_existing_socket(&socket, uid), Ok(false));
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
        assert_eq!(check_existing_socket(&socket, uid), Ok(true));
        assert!(check_socket_path(&socket, uid).is_ok());
        assert!(check_existing_socket(&socket, uid.wrapping_add(1)).is_err());
        drop(listener);
        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_dir(&dir);
    }
}
