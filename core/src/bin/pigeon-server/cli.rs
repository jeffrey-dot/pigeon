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
    /// Bridge stdin/stdout to an already running host. `ssh host pigeon-server --proxy` makes
    /// the ssh channel the transport; this mode knows nothing about the protocol.
    Proxy,
    /// Start a host in the background, the way `tmux` starts its server, and return:
    /// a fresh process group so the starter's exit (or `kill -9`) does not take it along.
    Detach,
}

pub struct Options {
    pub socket: PathBuf,
    pub shell: String,
    pub transport: Transport,
    /// Where a detached host writes its log (`--log`); none = discarded.
    pub log: Option<PathBuf>,
}

pub enum Command {
    Run(Options),
    /// Print this and exit (`--help`, `--socket-path`).
    Print(String),
}

const USAGE: &str = "usage: pigeon-server [--socket PATH] [--shell PATH] [--stdio|--proxy]
  --socket-path  print the default socket path and exit
  --version  print the version and exit
  --stdio  also serve one client on stdin/stdout (start a remote host)
  --proxy  bridge stdin/stdout to a running host's socket
  --detach  start a host in the background (own process group) and return
  --log PATH  where a --detach host writes its log
";

pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut options = Options {
        socket: default_socket_path(),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()),
        transport: Transport::Socket,
        log: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => {
                options.socket = PathBuf::from(args.next().ok_or("--socket needs a path")?)
            }
            "--shell" => options.shell = args.next().ok_or("--shell needs a path")?,
            "--log" => options.log = Some(PathBuf::from(args.next().ok_or("--log needs a path")?)),
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
            "--detach" => options.transport = Transport::Detach,
            // The client reads the protocol off this line to decide whether an installed
            // server can be used as is (`docs/remote.md`).
            "--version" => return Ok(Command::Print(version_line())),
            "-h" | "--help" => return Ok(Command::Print(USAGE.to_owned())),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Command::Run(options))
}

/// `pigeon-server <version> proto <major>.<minor>`, one line. The client reuses any installed
/// server whose protocol major matches its own, so the protocol is what this line must carry.
pub fn version_line() -> String {
    let proto = pigeon_core::api::PROTO_VERSION;
    format!(
        "pigeon-server {} proto {}.{}\n",
        pigeon_core::api::VERSION,
        proto >> 8,
        proto & 0xff
    )
}

/// Where the socket lives (protocol §1, `docs/remote.md` §2):
///
/// - a host installed by the client (`~/.pigeon/server/<version>/pigeon-server`) uses
///   `~/.pigeon/state/pigeon.sock`. `$XDG_RUNTIME_DIR` is removed by systemd when the last
///   login session ends, which would orphan a host that is meant to outlive the ssh link;
/// - otherwise `$XDG_RUNTIME_DIR/pigeon.sock`, else `/tmp/pigeon-<uid>/pigeon.sock` (a
///   private directory like tmux's `/tmp/tmux-<uid>/`, so nobody else can plant a socket
///   at the path before us).
fn default_socket_path() -> PathBuf {
    let executable = std::env::current_exe().ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(state) = state_dir_for(executable.as_deref(), home.as_deref()) {
        return state.join("pigeon.sock");
    }
    runtime_socket_path()
}

/// `~/.pigeon/state` when `executable` sits under `~/.pigeon/server/`; `None` otherwise (a
/// development build, a host on this Mac). Pure, so the rule is unit-tested.
pub fn state_dir_for(executable: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let root = home?.join(".pigeon");
    executable?
        .starts_with(root.join("server"))
        .then(|| root.join("state"))
}

fn runtime_socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("pigeon.sock");
    }
    let uid = crate::socket_trust::current_uid();
    PathBuf::from(format!("/tmp/pigeon-{uid}/pigeon.sock"))
}

#[cfg(test)]
mod tests {
    use super::{parse, state_dir_for, version_line, Command};
    use std::path::{Path, PathBuf};

    #[test]
    fn version_prints_the_crate_and_protocol_versions() {
        let Ok(Command::Print(text)) = parse(["--version".to_owned()].into_iter()) else {
            panic!("--version prints");
        };
        assert_eq!(
            text,
            format!("pigeon-server {} proto 1.0\n", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(text, version_line());
    }

    #[test]
    fn a_misspelt_version_flag_is_an_error() {
        assert!(parse(["--versio".to_owned()].into_iter()).is_err());
    }

    #[test]
    fn an_installed_remote_host_keeps_its_socket_under_dot_pigeon() {
        let home = Path::new("/home/ada");
        let exe = Path::new("/home/ada/.pigeon/server/0.1.0/pigeon-server");
        assert_eq!(
            state_dir_for(Some(exe), Some(home)),
            Some(PathBuf::from("/home/ada/.pigeon/state"))
        );
    }

    #[test]
    fn any_other_binary_keeps_the_runtime_dir() {
        let home = Path::new("/home/ada");
        for exe in [
            "/Users/ada/dev/pigeon/core/target/release/pigeon-server",
            "/home/ada/.pigeon/hostile/pigeon-server",
            "/home/ada/.pigeonx/host/pigeon-server",
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
