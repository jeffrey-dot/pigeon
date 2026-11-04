//! Workspace input targets and window actions.

use super::Workspace;
use crate::input::{self, Action};
use crate::render::Renderer;
use crate::screen::Screen;
use gtk4::{glib, prelude::*};
use pigeon_core::api::{Message, SplitAxis};

const SPLIT_RATIO: f32 = 0.5;

impl Workspace {
    pub(super) fn update_input(&self) {
        let pane = self.focus().filter(|pane| {
            self.panes
                .get(pane)
                .is_some_and(|screen| screen.exited.is_none())
        });
        self.input.set_target(pane);
        let Some(screen) = pane.and_then(|pane| self.panes.get(&pane)) else {
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
        if let Some(pane) = self.focus() {
            self.return_live(pane);
        }
    }

    pub fn send_input(&mut self, message: Message) {
        match &message {
            Message::Keys { pane_id, .. } | Message::Paste { pane_id, .. } => {
                self.return_live(*pane_id)
            }
            _ => return,
        }
        self.host.send(message);
    }

    pub fn copy_screen(&self) -> Option<String> {
        self.focus()
            .and_then(|pane| self.panes.get(&pane))
            .map(Screen::visible_text)
    }

    pub fn action(&mut self, action: Action) {
        let Some(pane) = self.focus() else {
            return;
        };
        self.request += 1;
        let id = self.request;
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
            Action::Zoom if self.zoom == Some(pane) => Message::Unzoom { id, pane_id: pane },
            Action::Zoom => Message::ZoomPane { id, pane_id: pane },
            Action::CopyScreen | Action::Paste => return,
        };
        self.host.send(message);
    }

    pub fn key(
        &mut self,
        key: gtk4::gdk::Key,
        modifiers: gtk4::gdk::ModifierType,
        handled: bool,
        committed: bool,
    ) -> glib::Propagation {
        let Some(pane) = self.focus() else {
            return glib::Propagation::Proceed;
        };
        let Some(screen) = self
            .panes
            .get(&pane)
            .filter(|screen| screen.exited.is_none())
        else {
            return glib::Propagation::Stop;
        };
        if let Some(bytes) =
            input::fallback(key, modifiers, screen.app_cursor(), handled, committed)
        {
            self.send_input(Message::Keys {
                pane_id: pane,
                bytes,
            });
            return glib::Propagation::Stop;
        }
        if handled || committed {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }
}
