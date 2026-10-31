//! What the host broadcasts to every client task; each one filters for itself.

use pigeon_core::proto::Message;

#[derive(Clone)]
pub enum Event {
    Delta {
        pane_id: u32,
        message: Message,
    },
    Exited {
        pane_id: u32,
        code: i32,
    },
    Layout {
        session_id: u32,
        message: Message,
    },
    /// A pane's size changed: who won the policy and how many clients watch (protocol §6).
    SizeState {
        session_id: u32,
        message: Message,
    },
    Status {
        pane_id: u32,
        message: Message,
    },
    /// Titles/cwd changed: clients should refresh `sessions`.
    SessionsChanged,
    /// The pane's grid changed size. Delta rows cannot say that, so watchers get a fresh
    /// snapshot instead (which also resets their `seq` base).
    Resized {
        pane_id: u32,
    },
}
