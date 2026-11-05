//! Session selection and screen routing for protocol §3.2–3.3; no GTK or I/O.

use crate::screen::Screen;
use pigeon_core::api::{
    layout_focus, layout_rects, layout_splits, Message, PaneId, Rect, SessionInfo, Split,
};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct SessionModel {
    pub sessions: Vec<SessionInfo>,
    pub selected: Option<u32>,
    pub panes: HashMap<PaneId, Screen>,
    pub layout: Vec<(PaneId, Rect)>,
    pub splits: Vec<Split>,
    pub zoom: Option<PaneId>,
    pub focus: Option<PaneId>,
    pub connected: bool,
    pub scrollback: bool,
    pub selection_generation: u64,
    pub received: HashSet<PaneId>,
    barrier: Option<u64>,
    attached: HashSet<PaneId>,
    request: u32,
    creating: Option<u32>,
    initial_list: bool,
    restore_session: Option<u32>,
    resume_selection: Option<u32>,
    resume_candidates: HashSet<PaneId>,
    resume_wait: HashMap<u64, PaneId>,
    pub caught_up: HashSet<PaneId>,
}

impl SessionModel {
    pub fn disable_resume(&mut self) {
        self.resume_candidates.clear();
        self.panes.clear();
    }
    pub fn new(scrollback: bool) -> Self {
        Self {
            connected: true,
            scrollback,
            ..Self::default()
        }
    }
    pub fn request_id(&mut self) -> u32 {
        self.request = self.request.saturating_add(1);
        self.request
    }

    pub fn selected_session(&self) -> Option<&SessionInfo> {
        self.sessions
            .iter()
            .find(|session| Some(session.session_id) == self.selected)
    }

    pub fn knows(&self, pane: PaneId) -> bool {
        self.sessions
            .iter()
            .any(|session| session.panes.contains(&pane))
    }

    pub fn focused_pane(&self) -> Option<PaneId> {
        self.focus.filter(|pane| {
            self.connected
                && self.barrier.is_none()
                && (self.received.contains(pane) || self.caught_up.contains(pane))
                && self.layout.iter().any(|(id, _)| id == pane)
                && self.panes.contains_key(pane)
                && self.zoom.is_none_or(|zoom| zoom == *pane)
        })
    }

    pub fn input_pane(&self) -> Option<PaneId> {
        self.focused_pane()
            .filter(|pane| self.panes.get(pane).is_some_and(|s| s.exited.is_none()))
    }

    pub fn disconnect(&mut self) {
        self.connected = false;
        self.received.clear();
        self.caught_up.clear();
        self.resume_candidates = self
            .panes
            .iter()
            .filter_map(|(id, screen)| screen.resume_seq().map(|_| *id))
            .collect();
    }

    /// A handshake is not a fresh picture. Keep selection intent only within one instance.
    pub fn reconnect(&mut self, same_instance: bool, scrollback: bool) {
        self.restore_session = if same_instance { self.selected } else { None };
        self.resume_selection = self.restore_session;
        if !same_instance {
            self.resume_candidates.clear();
        }
        self.resume_wait.clear();
        self.caught_up.clear();
        self.connected = true;
        self.scrollback = scrollback;
        self.selection_generation = self.selection_generation.wrapping_add(1);
        self.received.clear();
        self.attached.clear();
        self.barrier = None;
        self.creating = None;
        self.initial_list = true;
        self.selected = None;
        self.focus = None;
        self.zoom = None;
        self.layout.clear();
        self.splits.clear();
        self.panes
            .retain(|id, _| self.resume_candidates.contains(id));
        self.sessions.clear();
    }

    pub fn new_session(&mut self, cwd: Option<String>) -> Vec<Message> {
        if !self.connected || self.creating.is_some() {
            return vec![];
        }
        let id = self.request_id();
        self.creating = Some(id);
        vec![Message::NewSession {
            id,
            cwd,
            shell: None,
            env: None,
        }]
    }

    pub fn close_selected(&mut self) -> Vec<Message> {
        let panes = self
            .selected_session()
            .map(|s| s.panes.clone())
            .unwrap_or_default();
        panes
            .into_iter()
            .map(|pane_id| Message::ClosePane {
                id: self.request_id(),
                pane_id,
            })
            .collect()
    }

    pub fn select(&mut self, session: u32) -> Vec<Message> {
        if !self.connected || self.selected == Some(session) {
            return vec![];
        }
        let Some(info) = self.sessions.iter().find(|info| info.session_id == session) else {
            return vec![];
        };
        let resuming = self.resume_selection == Some(session);
        self.resume_selection = None;
        if !resuming {
            self.resume_candidates.clear();
            self.resume_wait.clear();
        }
        self.caught_up.clear();
        let pane = info
            .active_pane
            .filter(|pane| info.panes.contains(pane))
            .or_else(|| info.panes.first().copied());
        self.selection_generation = self.selection_generation.wrapping_add(1);
        self.received.clear();
        self.selected = Some(session);
        self.layout.clear();
        self.splits.clear();
        self.zoom = None;
        self.focus = None;
        self.attached.clear();
        // Cached history is not evidence for the newly selected live view.
        if let Some(session) = self.selected_session() {
            let panes = session.panes.clone();
            for pane in panes {
                if !self.resume_candidates.contains(&pane) {
                    self.panes.remove(&pane);
                }
            }
        }
        let ts = u64::MAX - self.selection_generation;
        self.barrier = Some(ts);
        let mut effects = vec![Message::Ping { ts }];
        effects.extend(pane.map(|pane| self.attach(pane)).unwrap_or_default());
        effects
    }

    pub fn attach(&mut self, pane_id: PaneId) -> Vec<Message> {
        self.attached.insert(pane_id);
        if self.resume_candidates.remove(&pane_id) {
            if let Some(since) = self.panes.get(&pane_id).and_then(Screen::resume_seq) {
                let id = self.request_id();
                let ts = (1u64 << 63) | u64::from(id);
                self.resume_wait.insert(ts, pane_id);
                return vec![
                    Message::Attach {
                        id,
                        pane_id,
                        size_policy: None,
                        history: Some(false),
                        since: Some(since),
                    },
                    Message::Ping { ts },
                ];
            }
        }
        let mut effects = vec![];
        if self.scrollback {
            if let Some(screen) = self.panes.get_mut(&pane_id) {
                screen.scroll.request(0);
            }
            effects.push(Message::Scroll { pane_id, offset: 0 });
        }
        effects.push(Message::Attach {
            id: self.request_id(),
            pane_id,
            size_policy: None,
            history: Some(false),
            since: None,
        });
        effects
    }

    pub fn apply(&mut self, message: &Message) -> Vec<Message> {
        match message {
            Message::Pong { ts } if self.barrier == Some(*ts) => self.barrier = None,
            Message::Pong { ts } => {
                if let Some(pane) = self.resume_wait.remove(ts) {
                    if self.panes.contains_key(&pane) {
                        self.caught_up.insert(pane);
                    }
                }
            }
            Message::Sessions { items } => {
                self.sessions = items.clone();
                self.sessions.sort_by_key(|s| s.session_id);
                if let Some(restore) = self
                    .restore_session
                    .take()
                    .filter(|id| self.sessions.iter().any(|s| s.session_id == *id))
                {
                    return self.select(restore);
                }
                let known: HashSet<_> =
                    items.iter().flat_map(|s| s.panes.iter().copied()).collect();
                self.panes.retain(|pane, _| known.contains(pane));
                self.received.retain(|pane| known.contains(pane));
                self.attached.retain(|pane| known.contains(pane));
                self.layout.retain(|(pane, _)| known.contains(pane));
                let first = !self.initial_list;
                self.initial_list = true;
                if self.selected_session().is_none() {
                    self.selected = None;
                    self.layout.clear();
                    self.splits.clear();
                    self.focus = None;
                    self.zoom = None;
                    if let Some(session) = self.sessions.first() {
                        return self.select(session.session_id);
                    }
                    if first {
                        return self.new_session(None);
                    }
                }
            }
            Message::Created {
                id,
                session_id,
                pane_id,
            } if self.creating == Some(*id) => {
                self.creating = None;
                if !self.sessions.iter().any(|s| s.session_id == *session_id) {
                    self.sessions.push(SessionInfo {
                        session_id: *session_id,
                        title: "New session".into(),
                        cwd: String::new(),
                        panes: vec![*pane_id],
                        active_pane: Some(*pane_id),
                        active_title: None,
                        active_program: None,
                    });
                }
                let mut effects = self.select(*session_id);
                effects.push(Message::Sessions { items: vec![] });
                return effects;
            }
            Message::Error { id, .. } if id.is_some() && *id == self.creating => {
                self.creating = None
            }
            Message::Error { id: Some(id), .. } => {
                self.resume_wait.remove(&((1u64 << 63) | u64::from(*id)));
            }
            Message::Layout {
                session_id, zoom, ..
            } if Some(*session_id) == self.selected && self.barrier.is_none() => {
                let Ok(rects) = layout_rects(message) else {
                    return vec![];
                };
                self.layout = rects.into_iter().filter(|(pane, _)| *pane != 0).collect();
                self.splits = layout_splits(message).unwrap_or_default();
                self.zoom = *zoom;
                self.focus = layout_focus(message)
                    .ok()
                    .flatten()
                    .filter(|pane| self.layout.iter().any(|(id, _)| id == pane))
                    .or_else(|| self.layout.first().map(|(pane, _)| *pane));
                if let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.session_id == *session_id)
                {
                    session.panes = self.layout.iter().map(|(pane, _)| *pane).collect();
                    session.active_pane = self.focus;
                }
                let known: HashSet<_> = self
                    .sessions
                    .iter()
                    .flat_map(|s| s.panes.iter().copied())
                    .collect();
                self.panes.retain(|pane, _| known.contains(pane));
                self.received.retain(|pane| known.contains(pane));
                let missing: Vec<_> = self
                    .layout
                    .iter()
                    .map(|(pane, _)| *pane)
                    .filter(|pane| !self.attached.contains(pane))
                    .collect();
                let mut effects: Vec<_> = missing
                    .into_iter()
                    .flat_map(|pane| self.attach(pane))
                    .collect();
                if self.layout.is_empty() {
                    effects.push(Message::Sessions { items: vec![] });
                }
                return effects;
            }
            Message::Snapshot { pane_id, .. } if self.knows(*pane_id) && self.barrier.is_none() => {
                let applied = if let Some(screen) = self.panes.get_mut(pane_id) {
                    let before = screen.revision;
                    screen.apply_snapshot(message) && screen.revision != before
                } else if matches!(
                    message,
                    Message::Snapshot {
                        offset: Some(_),
                        ..
                    }
                ) {
                    false
                } else if let Some(screen) = Screen::from_message(message) {
                    self.panes.insert(*pane_id, screen);
                    true
                } else {
                    false
                };
                if applied {
                    self.received.insert(*pane_id);
                }
            }
            Message::Delta { pane_id, .. } if self.knows(*pane_id) => {
                let before = self.panes.get(pane_id).map(|s| s.revision);
                let applied = self
                    .panes
                    .get_mut(pane_id)
                    .is_some_and(|screen| screen.apply_delta(message));
                if applied
                    && self
                        .panes
                        .get(pane_id)
                        .is_some_and(|s| Some(s.revision) != before)
                {
                    self.received.insert(*pane_id);
                }
                if !applied
                    && (self.layout.iter().any(|(pane, _)| pane == pane_id)
                        || self.resume_wait.values().any(|pane| pane == pane_id))
                {
                    self.resume_wait.retain(|_, pane| pane != pane_id);
                    self.caught_up.remove(pane_id);
                    return self.attach(*pane_id);
                }
            }
            Message::PaneExited { pane_id, code } => {
                if let Some(screen) = self.panes.get_mut(pane_id) {
                    screen.exited = Some(*code);
                }
            }
            _ => {}
        }
        vec![]
    }
}
