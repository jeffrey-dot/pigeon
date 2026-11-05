//! What the host remembers about one connection.

use std::collections::HashMap;

use tokio::sync::mpsc::WeakSender;

use crate::pane::cmd::Cmd;

#[derive(Default)]
pub struct ClientState {
    /// Connection id this host assigned in `hello_ack`, unique per connection.
    pub client_id: u32,
    pub greeted: bool,
    /// Panes this connection gets live output for → negotiated `delta`.
    pub watched: HashMap<u32, bool>,
    /// The session whose layout this connection follows (one per window in M1).
    pub session: Option<u32>,
    /// Internal layout/size events emitted before this subscription are obsolete.
    pub control_since: u64,
    /// Whether the handshake negotiated `delta` for this connection.
    pub delta_ok: bool,
    /// `status_all`: this connection gets every pane's `status`, attached or not, so a
    /// sidebar can follow all agents without joining (and resizing) every session.
    pub status_all: bool,
    /// Complete multi-record sets replace legacy status when negotiated.
    pub status_snapshot: bool,
    pub text_query: bool,
    /// How far each pane is scrolled back for *this* window; 0 is the live screen. Two
    /// windows on one session scroll independently because this lives per connection.
    pub views: HashMap<u32, usize>,
    /// Frames a request produced beyond its one reply (an `attach.since` replay).
    pub outbox: Vec<pigeon_core::proto::Message>,
    /// The pane this connection last typed into and its queue, so a keystroke does not
    /// search every session under the global lock. Weak: a cached handle must not keep a
    /// closed pane's task alive (it ends when its last sender goes).
    pub typing: Option<(u32, WeakSender<Cmd>)>,
    /// `HostState::note_activity`'s cache: typing into this pane at this vote epoch
    /// changed nothing.
    pub settled: Option<(u32, u64)>,
}

impl ClientState {
    pub fn new(client_id: u32) -> Self {
        Self {
            client_id,
            ..Default::default()
        }
    }

    /// Whether this window is reading history in `pane_id` rather than the live screen.
    pub fn scrolled_back(&self, pane_id: u32) -> bool {
        self.views.get(&pane_id).copied().unwrap_or(0) > 0
    }
}
