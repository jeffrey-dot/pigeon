//! Session host: owns the PTYs and outlives every client window.
//!
//! One tokio runtime, one task per pane (that task is the only owner of the
//! pane's terminal state), one task per client. A client disconnecting only drops
//! its task — the PTYs and the layout keep running, which is the point of pigeon.

mod cli;
mod client;
mod client_state;
mod cmd;
mod dispatch;
mod dispatch_layout;
mod dispatch_pane;
mod event;
mod handshake;
mod host_state;
mod pane_actor;
mod pane_query;
mod transport;

use std::process::ExitCode;

use cli::{Command, Transport};
use host_state::HostState;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("pigeon-host: cannot start runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pigeon-host: {error}");
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
    let host = HostState::new(options.shell);
    transport::serve(&options.socket, options.transport, host).await
}
