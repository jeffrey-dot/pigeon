//! Command line: where the socket lives, which shell to spawn, how clients reach us.

use std::path::{Path, PathBuf};

/// How clients reach this host (docs/remote.md §2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Unix socket only: the local, always-on case.
    Socket,
    /// Socket *and* stdin/stdout: how a remote host is started over `ssh`, so the same
    /// process serves the ssh channel now and later bridges from the socket.
    Stdio,
    /// Bridge stdin/stdout to an already running host. `ssh host pigeon-host --proxy` makes
    /// the ssh channel the transport; this mode knows nothing about the protocol.
    Proxy,
}

pub struct Options {
    pub socket: PathBuf,
    pub shell: String,
    pub transport: Transport,
}

pub enum Command {
    Run(Options),
    /// Print this and exit (`--help`, `--socket-path`).
    Print(String),
}

const USAGE: &str = "usage: pigeon-host [--socket PATH] [--shell PATH] [--stdio|--proxy]
  --socket-path  print the default socket path and exit
  --stdio  also serve one client on stdin/stdout (start a remote host)
  --proxy  bridge stdin/stdout to a running host's socket
";

pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut options = Options {
        socket: default_socket_path(),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()),
        transport: Transport::Socket,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => {
                options.socket = PathBuf::from(args.next().ok_or("--socket needs a path")?)
            }
            "--shell" => options.shell = args.next().ok_or("--shell needs a path")?,
            "--stdio" => options.transport = Transport::Stdio,
            // Prints where the socket would be and exits: the client needs the exact path to
            // set up `ssh -L` without duplicating the rule (protocol §1).
            "--socket-path" => {
                return Ok(Command::Print(format!(
                    "{}\n",
                    default_socket_path().display()
                )))
            }
            "--proxy" => options.transport = Transport::Proxy,
            "-h" | "--help" => return Ok(Command::Print(USAGE.to_owned())),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Command::Run(options))
}

/// Where the socket lives (protocol §1, `docs/remote.md` §2):
///
/// - a host installed by the client (`~/.pigeon/host/<version>/pigeon-host`) uses
///   `~/.pigeon/state/pigeon.sock`. `$XDG_RUNTIME_DIR` is removed by systemd when the last
///   login session ends, which would orphan a host that is meant to outlive the ssh link;
/// - otherwise `$XDG_RUNTIME_DIR/pigeon.sock`, else `/tmp/pigeon-<uid>.sock`.
fn default_socket_path() -> PathBuf {
    let executable = std::env::current_exe().ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(state) = state_dir_for(executable.as_deref(), home.as_deref()) {
        return state.join("pigeon.sock");
    }
    runtime_socket_path()
}

/// `~/.pigeon/state` when `executable` sits under `~/.pigeon/host/`; `None` otherwise (a
/// development build, a host on this Mac). Pure, so the rule is unit-tested.
pub fn state_dir_for(executable: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let root = home?.join(".pigeon");
    executable?
        .starts_with(root.join("host"))
        .then(|| root.join("state"))
}

/// shortcut: the uid comes from `id -u` because `getuid` needs `unsafe`, which
/// is banned outside `ffi.rs`; swap to a safe syscall wrapper when one is added.
fn runtime_socket_path() -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::state_dir_for;
    use std::path::{Path, PathBuf};

    #[test]
    fn an_installed_remote_host_keeps_its_socket_under_dot_pigeon() {
        let home = Path::new("/home/ada");
        let exe = Path::new("/home/ada/.pigeon/host/0.1.0/pigeon-host");
        assert_eq!(
            state_dir_for(Some(exe), Some(home)),
            Some(PathBuf::from("/home/ada/.pigeon/state"))
        );
    }

    #[test]
    fn any_other_binary_keeps_the_runtime_dir() {
        let home = Path::new("/home/ada");
        for exe in [
            "/Users/ada/dev/pigeon/core/target/release/pigeon-host",
            "/home/ada/.pigeon/hostile/pigeon-host",
            "/home/ada/.pigeonx/host/pigeon-host",
        ] {
            assert_eq!(
                state_dir_for(Some(Path::new(exe)), Some(home)),
                None,
                "{exe}"
            );
        }
        assert_eq!(state_dir_for(None, Some(home)), None);
        assert_eq!(state_dir_for(Some(Path::new("/x")), None), None);
    }
}
