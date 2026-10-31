//! Everything the client tasks share: the sessions, the event bus, the pane metadata and
//! the palette, plus the session-wide steps that touch more than one of them.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Mutex};

use pigeon_core::session::{PaneMeta, SessionState};
use pigeon_core::term::Palette;

use crate::cmd::{send_or_queue, Cmd};
use crate::event::Event;

/// One session, with a channel to each pane's task as the pane handle.
pub type Session = SessionState<mpsc::Sender<Cmd>>;
pub type Sessions = Arc<Mutex<HashMap<u32, Session>>>;
pub type Meta = Arc<Mutex<HashMap<u32, PaneMeta>>>;
/// The colours the panes answer `OSC 4/10/11` with: harp's built-in dark theme until a client
/// sends `appearance`, then whatever that client last reported.
/// Exit codes of panes whose child is gone but that nobody has closed yet.
pub type Exits = Arc<std::sync::Mutex<HashMap<u32, i32>>>;
pub type PaletteSlot = Arc<std::sync::Mutex<Option<Palette>>>;

#[derive(Clone)]
pub struct HostState {
    pub sessions: Sessions,
    pub events: broadcast::Sender<Event>,
    pub meta: Meta,
    pub palette: PaletteSlot,
    pub exits: Exits,
    /// The shell new sessions get unless they ask for another.
    pub shell: String,
}

impl HostState {
    pub fn new(shell: String) -> Self {
        let (events, _) = broadcast::channel::<Event>(256);
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            events,
            meta: Arc::new(Mutex::new(HashMap::new())),
            // Before a client speaks, answer colour queries with harp's built-in dark theme.
            palette: Arc::new(std::sync::Mutex::new(Some(Palette::default_dark()))),
            exits: Arc::new(std::sync::Mutex::new(HashMap::new())),
            shell,
        }
    }

    /// The palette a new pane starts with: the last `appearance` the client sent, or harp's
    /// built-in dark theme before any client has spoken.
    pub fn current_palette(&self) -> Option<Palette> {
        self.palette.lock().ok().and_then(|slot| *slot)
    }

    /// The exit code of `pane_id` if its child is gone and the pane not yet closed.
    pub fn exit_code(&self, pane_id: u32) -> Option<i32> {
        self.exits.lock().ok()?.get(&pane_id).copied()
    }

    /// Protocol §6 `latest`: whoever last focused, typed or resized owns the size.
    pub async fn note_activity(&self, client_id: u32, pane_id: u32) {
        let mut map = self.sessions.lock().await;
        let Some(session_id) = session_of(&map, pane_id) else {
            return;
        };
        let Some(session) = map.get_mut(&session_id) else {
            return;
        };
        if session.vote.activity(client_id) && session.refit() {
            apply_layout(session_id, session, &self.events);
        }
    }

    /// Drops a closed connection from every session and re-fits whatever the remaining
    /// clients ask for.
    pub async fn forget_client(&self, client_id: u32) {
        let mut map = self.sessions.lock().await;
        for (session_id, session) in map.iter_mut() {
            if session.vote.leave(client_id) && session.refit() {
                apply_layout(*session_id, session, &self.events);
            }
        }
    }
}

pub fn session_of(map: &HashMap<u32, Session>, pane_id: u32) -> Option<u32> {
    map.iter()
        .find(|(_, session)| session.panes.contains_key(&pane_id))
        .map(|(id, _)| *id)
}

/// Re-fits every pane to its rect and tells clients what the tree looks like.
pub fn apply_layout(session_id: u32, session: &Session, events: &broadcast::Sender<Event>) {
    for (pane_id, cols, rows) in session.pane_sizes() {
        if let Some(sender) = session.panes.get(&pane_id) {
            send_or_queue(sender, Cmd::Resize(cols, rows));
        }
    }
    let _ = events.send(Event::Layout {
        session_id,
        message: session.layout_message(session_id),
    });
    broadcast_size(session_id, session, events);
}

/// Protocol §6: every size change is announced with its `source` and the number of
/// clients, so each window can show who is in charge.
pub fn broadcast_size(session_id: u32, session: &Session, events: &broadcast::Sender<Event>) {
    let clients = session.vote.clients();
    if clients > 1 && std::env::var_os("PIGEON_DEBUG").is_some() {
        let (cols, rows) = session.size();
        eprintln!(
            "pigeon-host: size_state session={session_id} clients={clients} source={} box={cols}x{rows}",
            session.vote.policy().as_str(),
        );
    }
    for message in session.size_states() {
        let _ = events.send(Event::SizeState {
            session_id,
            message,
        });
    }
}
