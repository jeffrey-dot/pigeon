//! Requests to one pane's task, the only owner of that pane's terminal state.

use tokio::sync::{mpsc, oneshot};

use pigeon_core::proto::Message;
use pigeon_core::term::Palette;

pub enum Cmd {
    Input(Vec<u8>),
    /// Clipboard text; the pane decides on bracketing because only it knows the mode.
    Paste(String),
    Resize(u16, u16),
    /// The user has seen this status record.
    Ack(u32),
    /// Terminate the child (protocol `close_pane`).
    Kill,
    Snapshot {
        history: bool,
        reply: oneshot::Sender<Message>,
    },
    /// Deltas after `since`, or a snapshot when the backlog no longer has them all.
    Since {
        since: u32,
        reply: oneshot::Sender<Vec<Message>>,
    },
    /// Look `offset` lines into the history; the reply carries the view and the applied
    /// offset (protocol §3.3 `scroll`).
    Scroll {
        offset: usize,
        reply: oneshot::Sender<(Message, usize)>,
    },
    LatestStatus {
        reply: oneshot::Sender<Option<Message>>,
    },
    /// The client's colours changed: answer the panes' `OSC 4/10/11` queries with these.
    SetPalette(Option<Palette>),
}

/// Hands `command` to a pane without blocking the caller (which holds the sessions lock)
/// and without losing it: a full queue gets the command from a task that waits its turn.
/// A lost `Resize` leaves the PTY at the wrong size, a lost `Kill` an orphaned process.
pub fn send_or_queue(sender: &mpsc::Sender<Cmd>, command: Cmd) {
    match sender.try_send(command) {
        Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
        Err(mpsc::error::TrySendError::Full(command)) => {
            let sender = sender.clone();
            tokio::spawn(async move {
                let _ = sender.send(command).await;
            });
        }
    }
}
