//! What the host broadcasts to every client task; each one filters for itself.
//!
//! Messages that every interested client gets byte for byte are encoded once by the sender,
//! so ten windows on one busy pane do not encode each delta ten times.

use pigeon_core::proto::Message;

use crate::out_queue::Frame;

#[derive(Clone)]
pub enum Event {
    Delta {
        pane_id: u32,
        frame: Frame,
    },
    Exited {
        pane_id: u32,
        code: i32,
    },
    Layout {
        session_id: u32,
        frame: Frame,
    },
    /// A pane's size changed: who won the policy and how many clients watch (protocol §6).
    SizeState {
        session_id: u32,
        frame: Frame,
    },
    Status {
        pane_id: u32,
        frame: Frame,
    },
    /// Titles/cwd changed: clients should refresh `sessions`.
    SessionsChanged,
    /// The pane's grid changed size. Delta rows cannot say that, so watchers get a fresh
    /// snapshot instead (which also resets their `seq` base).
    Resized {
        pane_id: u32,
    },
}

/// `message` as a whole frame, or `None` (logged) if it cannot be one: the only failure is a
/// frame over the 8 MiB limit, which no client could read anyway.
pub fn encode(message: &Message) -> Option<Frame> {
    crate::outbound::encode(message)
        .map_err(|error| eprintln!("pigeon-host: {error}"))
        .ok()
}
