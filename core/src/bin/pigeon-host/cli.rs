//! Command line: where the socket lives, which shell to spawn, how clients reach us.

use std::path::PathBuf;

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
