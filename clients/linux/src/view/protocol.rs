//! Routing effects and presentation checks at the GTK / model seam.
use super::Workspace;
use gtk4::prelude::*;
use pigeon_core::api::Message;

impl Workspace {
    pub fn send_effects(&self, effects: Vec<Message>) {
        for mut message in effects {
            if let Message::NewSession { cwd, .. } = &mut message {
                if cwd.is_none() {
                    *cwd = std::env::current_dir()
                        .ok()
                        .and_then(|p| p.to_str().map(str::to_owned));
                }
            }
            self.host.send(message);
        }
    }

    pub fn apply(&mut self, message: &Message) {
        let previous = self.model.selected;
        let previous_focus = self.model.focus;
        if let Message::SizeState {
            pane_id,
            cols,
            rows,
            source,
            clients,
        } = message
        {
            self.sizes.insert(*pane_id, (source.clone(), *clients));
            if self.model.layout.iter().any(|(pane, _)| pane == pane_id)
                && self.model.panes.get(pane_id).is_some_and(|s| {
                    s.cols() != usize::from(*cols) || s.rows() != usize::from(*rows)
                })
            {
                self.return_live(*pane_id);
            }
        }
        let effects = self.model.apply(message);
        if previous != self.model.selected {
            self.reset_selection();
        }
        if previous_focus != self.model.focus {
            self.wheel.reset();
        }
        self.send_effects(effects);
        // Status may precede the next sessions broadcast for a newly created background pane.
        self.status.apply(message);
        if matches!(message, Message::Sessions { .. } | Message::Layout { .. }) {
            let known = self
                .model
                .sessions
                .iter()
                .flat_map(|s| s.panes.iter().copied())
                .collect();
            self.status.retain(&known);
            self.sizes.retain(|pane, _| known.contains(pane));
        }
        if matches!(message, Message::Layout { session_id, .. } if Some(*session_id) == self.model.selected)
        {
            self.presented.clear();
        }
        self.update_input();
        self.update_scroll();
        self.area.queue_render();
    }

    fn reset_selection(&mut self) {
        self.input.set_target(None);
        self.presented.clear();
        self.sized.clear();
        self.wheel.reset();
    }

    pub fn select(&mut self, session: u32) {
        if self.model.selected == Some(session) {
            return;
        }
        let effects = self.model.select(session);
        self.reset_selection();
        self.send_effects(effects);
        self.update_input();
        self.update_scroll();
        self.area.queue_render();
    }

    pub fn new_session(&mut self) {
        let cwd = self
            .model
            .selected_session()
            .map(|s| s.cwd.clone())
            .filter(|s| !s.is_empty());
        let effects = self.model.new_session(cwd);
        self.send_effects(effects);
    }

    pub fn close_session(&mut self, session: u32) {
        if self.model.selected != Some(session) || !self.model.connected {
            return;
        }
        let effects = self.model.close_selected();
        self.send_effects(effects);
    }

    pub fn disconnect(&mut self) {
        self.model.disconnect();
        self.input.set_target(None);
        self.presented.clear();
    }

    pub fn reveal(&mut self, pane: u32) {
        if !self
            .model
            .selected_session()
            .is_some_and(|s| s.panes.contains(&pane))
        {
            return;
        }
        if self.model.zoom.is_some_and(|zoom| zoom != pane) {
            let id = self.model.request_id();
            self.host.send(Message::Unzoom { id, pane_id: pane });
        }
        self.host.send(Message::Focus { pane_id: pane });
    }

    pub fn record_visible(&self, pane: u32) -> bool {
        self.model.connected
            && self.renderer.is_some()
            && self.area.is_mapped()
            && self.presented.contains(&pane)
            && self.model.received.contains(&pane)
            && self.model.layout.iter().any(|(id, _)| *id == pane)
            && self.model.zoom.is_none_or(|zoom| zoom == pane)
            && self
                .model
                .panes
                .get(&pane)
                .is_some_and(|s| !s.scroll.active() && s.cols() > 0 && s.rows() > 0)
    }
}
