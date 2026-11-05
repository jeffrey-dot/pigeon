//! Workspace input targets and window actions.

use super::Workspace;
use crate::input::{self, Action};
use crate::render::Renderer;
use crate::screen::Screen;
use gtk4::prelude::*;
use pigeon_core::api::{Message, SplitAxis};

const SPLIT_RATIO: f32 = 0.5;

impl Workspace {
    pub(super) fn update_input(&mut self) {
        let pane = self.focus().filter(|pane| {
            self.model
                .panes
                .get(pane)
                .is_some_and(|screen| screen.exited.is_none())
        });
        if self.input.target() != pane {
            self.cancel_input();
        }
        self.input.set_target(pane);
        self.input.set_modes(
            pane.and_then(|p| self.model.panes.get(&p))
                .map_or(0, |s| s.modes),
        );
        self.update_terminal_focus();
        let Some(screen) = pane.and_then(|pane| self.model.panes.get(&pane)) else {
            return;
        };
        let Some(metrics) = self.renderer.as_ref().map(Renderer::metrics) else {
            return;
        };
        let scale = self.scale.max(1.0);
        let frame = self
            .frames(
                (self.area.width() as f32 * scale) as i32,
                (self.area.height() as f32 * scale) as i32,
            )
            .into_iter()
            .find(|(id, _, _)| Some(*id) == pane);
        if let Some((_, origin, size)) = frame {
            self.input.set_cursor(input::cursor_rect(
                origin,
                size,
                [metrics.cell_w, metrics.cell_h],
                [screen.snapshot.cursor.x, screen.snapshot.cursor.y],
                scale,
            ));
        }
    }

    pub fn prepare_input(&mut self) {
        self.close_search(true);
        if let Some(pane) = self.focus() {
            self.return_live(pane);
        }
    }

    pub fn send_input(&mut self, message: Message) {
        self.close_search(true);
        match &message {
            Message::Keys { pane_id, .. } | Message::Paste { pane_id, .. } => {
                if self.model.input_pane() != Some(*pane_id) {
                    return;
                }
                self.return_live(*pane_id)
            }
            _ => return,
        }
        self.host.send(message);
    }

    pub fn copy_screen(&self) -> Option<String> {
        self.focus()
            .and_then(|pane| self.model.panes.get(&pane))
            .map(Screen::visible_text)
    }

    pub fn action(&mut self, action: Action) {
        let Some(pane) = self.focus() else {
            return;
        };
        let id = self.model.request_id();
        let message = match action {
            Action::SplitVertical | Action::SplitHorizontal => Message::SplitPane {
                id,
                pane_id: pane,
                axis: if action == Action::SplitVertical {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                },
                ratio: SPLIT_RATIO,
            },
            Action::Close => Message::ClosePane { id, pane_id: pane },
            Action::Zoom if self.model.zoom == Some(pane) => Message::Unzoom { id, pane_id: pane },
            Action::Zoom => Message::ZoomPane { id, pane_id: pane },
            Action::CopyScreen | Action::Paste => return,
        };
        self.host.send(message);
    }

    pub fn cancel_input(&mut self) {
        let bytes = self.input.cancel_keys();
        if let Some(pane_id) = self.input.target() {
            if !bytes.is_empty()
                && self.model.connected
                && self
                    .model
                    .panes
                    .get(&pane_id)
                    .is_some_and(|s| s.exited.is_none())
            {
                self.host.send(Message::Keys { pane_id, bytes });
            }
        }
        self.cancel_pointer();
    }

    pub fn send_key_release(&self, pane_id: u32, bytes: Vec<u8>) {
        // A press may already be queued behind return-live, before its snapshot arrives.
        if self.model.input_pane() == Some(pane_id) {
            self.host.send(Message::Keys { pane_id, bytes });
        }
    }

    pub fn send_report(&self, pane_id: u32, bytes: Vec<u8>) {
        if self.model.input_pane() == Some(pane_id)
            && self
                .model
                .panes
                .get(&pane_id)
                .is_some_and(|s| !s.scroll.active() && s.search_picture.is_none())
        {
            self.host.send(Message::Keys { pane_id, bytes });
        }
    }

    pub fn update_terminal_focus(&mut self) {
        let next = self.model.input_pane().filter(|pane| {
            self.area.has_focus()
                && self.model.panes.get(pane).is_some_and(|s| {
                    !s.scroll.active() && s.search_picture.is_none() && s.modes & 64 != 0
                })
        });
        if self.terminal_focus == next {
            return;
        }
        if let Some(pane_id) = self.terminal_focus.take() {
            if self.model.connected
                && self
                    .model
                    .panes
                    .get(&pane_id)
                    .is_some_and(|s| s.exited.is_none() && s.modes & 64 != 0)
            {
                self.host.send(Message::Keys {
                    pane_id,
                    bytes: b"\x1b[O".to_vec(),
                });
            }
        }
        self.terminal_focus = next;
        if let Some(pane_id) = next {
            self.send_report(pane_id, b"\x1b[I".to_vec());
        }
    }
}
