//! Plain-text hover and deliberate complete Ctrl-click; GIO is the sole URI opener.
use super::Workspace;
use crate::links::{self, LinkTarget};
use gtk4::{gdk::ModifierType as M, gio, prelude::*};

impl Workspace {
    pub(super) fn link_at(&self, pane: u32, point: [u16; 2]) -> Option<LinkTarget> {
        let screen = self.model.panes.get(&pane)?;
        if self.focus().is_none()
            || screen.search_picture.is_some()
            || !self.model.received.contains(&pane)
            || self.presented.get(&pane) != Some(&screen.revision)
        {
            return None;
        }
        let uri = screen
            .cell(usize::from(point[0]), usize::from(point[1]))?
            .hyperlink
            .as_deref()?;
        links::opening_uri(uri)?;
        Some(LinkTarget {
            pane,
            revision: screen.revision,
            column: point[0],
            row: point[1],
            uri: uri.to_owned(),
        })
    }
    pub(super) fn validate_link_press(&mut self) {
        let current = self.link_press.target().and_then(|t| {
            if self.focus() != Some(t.pane) {
                return None;
            }
            self.link_at(t.pane, [t.column, t.row])
        });
        self.link_press.validate(current.as_ref());
        self.refresh_hover(self.pointer.modifiers);
    }
    pub fn refresh_hover(&mut self, modifiers: M) {
        self.pointer.modifiers = modifiers;
        if let Some(axis) = self.dragging_divider() {
            self.divider_cursor(axis);
            return;
        }
        if let Some(split) = self
            .pointer
            .position
            .and_then(|(x, y)| self.divider_at(x, y))
        {
            self.divider_cursor(split.axis);
            return;
        }
        let target = self
            .pointer
            .position
            .and_then(|(x, y)| self.hit_cell(x, y))
            .and_then(|(pane, point)| self.link_at(pane, point));
        self.area.set_cursor_from_name(Some(
            if target.is_some() && modifiers.contains(M::CONTROL_MASK) {
                "pointer"
            } else {
                "text"
            },
        ));
        let tooltip = target.map(|t| format!("Ctrl-click to open: {}", t.uri)).unwrap_or_else(||
            "Ctrl+Shift+C: Copy visible pane screen (including scrollback)\nCtrl+Shift+V: Paste\nShift+PageUp/PageDown: Scroll\nShift+Home/End: Oldest / live".into());
        self.area.set_tooltip_text(Some(&tooltip));
    }
    pub(super) fn finish_link(&mut self, target: Option<&LinkTarget>, modifiers: M) {
        let context = self.area.display().app_launch_context();
        self.link_press
            .finish(target, modifiers.contains(M::CONTROL_MASK), |uri| {
                gio::AppInfo::launch_default_for_uri_async(
                    uri,
                    Some(&context),
                    None::<&gio::Cancellable>,
                    |result| {
                        if let Err(error) = result {
                            eprintln!("pigeon: could not open link: {error}");
                        }
                    },
                );
            });
    }
}
