//! What the host remembers about one connection.

use std::collections::HashMap;

#[derive(Default)]
pub struct ClientState {
    /// Connection id this host assigned in `hello_ack`, unique per connection.
    pub client_id: u32,
    pub greeted: bool,
    /// Panes this connection gets live output for → negotiated `delta`.
    pub watched: HashMap<u32, bool>,
    /// The session whose layout this connection follows (one per window in M1).
    pub session: Option<u32>,
    /// Whether the handshake negotiated `delta` for this connection.
    pub delta_ok: bool,
    /// `status_all`: this connection gets every pane's `status`, attached or not, so a
    /// sidebar can follow all agents without joining (and resizing) every session.
    pub status_all: bool,
    /// How far each pane is scrolled back for *this* window; 0 is the live screen. Two
    /// windows on one session scroll independently because this lives per connection.
    pub views: HashMap<u32, usize>,
    /// Frames a request produced beyond its one reply (an `attach.since` replay).
    pub outbox: Vec<pigeon_core::proto::Message>,
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
