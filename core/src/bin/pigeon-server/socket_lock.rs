//! One host per socket path. Probing the socket and then binding is a race: two hosts that
//! start together on a stale socket both find nobody listening, both remove it and both
//! bind, and the loser keeps writing the same `.state` file. An exclusive `flock` on
//! `<socket>.lock`, held for the life of the process, makes probe-and-bind one step; the
//! kernel drops it when the process dies, so a crash never leaves it stuck.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};

pub struct SocketLock {
    _held: Flock<File>,
}

pub fn lock_path(socket: &Path) -> PathBuf {
    socket.with_extension("lock")
}

impl SocketLock {
    /// Takes the lock without waiting: whoever holds it is (or is about to be) the host.
    pub fn acquire(socket: &Path) -> Result<Self, String> {
        let path = lock_path(socket);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|error| format!("open {path:?}: {error}"))?;
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(held) => Ok(Self { _held: held }),
            Err((_, Errno::EWOULDBLOCK)) => Err(format!(
                "another pigeon-server is already serving {socket:?}"
            )),
            Err((_, error)) => Err(format!("lock {path:?}: {error}")),
        }
    }
}
