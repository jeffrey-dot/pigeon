//! Session host: owns the PTYs and outlives every client window.
//!
//! One tokio runtime, one task per pane (that task is the only owner of the
//! pane's terminal state), a reader and a writer task per client. A client disconnecting
//! only drops its tasks — the PTYs and the layout keep running, which is the point of pigeon.

mod cli;
mod connection;
mod dispatch;
mod host;
mod pane;
mod socket_lock;
mod socket_trust;
mod transport;

use std::process::ExitCode;
use std::sync::OnceLock;

/// `PIGEON_DEBUG` set: trace frames and PTY bytes to stderr. Read once; the environment of
/// a running host does not change, and the check sits on every frame.
pub fn debug() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("PIGEON_DEBUG").is_some())
}

use cli::{Command, Transport};
use host::state::HostState;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("pigeon-server: cannot start runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pigeon-server: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let options = match cli::parse(std::env::args().skip(1))? {
        Command::Run(options) => options,
        Command::Print(text) => {
            print!("{text}");
            return Ok(());
        }
    };
    if options.transport == Transport::Proxy {
        return transport::proxy(&options.socket, &options.shell).await;
    }
    if options.transport == Transport::Detach {
        return transport::detach(&options.socket, &options.shell, options.log.as_deref()).await;
    }
    let host = HostState::new(options.shell);
    transport::serve(&options.socket, options.transport, host).await
}
