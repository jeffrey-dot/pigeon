//! Scroll UI and navigation. The host owns history, slicing and per-connection offsets.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::{self as gtk, gdk, glib, prelude::*};
use pigeon_core::api::{Message, PaneId};

use super::Workspace;
use crate::{input, render::Renderer, scroll_input};

impl Workspace {
    fn scroll(&mut self, pane: PaneId, offset: i32) {
        if !self.host.scrollback() {
            return;
        }
        let Some(screen) = self.model.panes.get_mut(&pane) else {
            return;
        };
        if screen.snapshot.alt && offset != 0 {
            return;
        }
        if screen.scroll.request(offset) {
            self.host.send(Message::Scroll {
                pane_id: pane,
                offset,
            });
            self.update_scroll();
        }
    }

    pub(super) fn return_live(&mut self, pane: PaneId) {
        self.wheel.reset();
        self.scroll(pane, 0);
    }

    pub fn scroll_key(&mut self, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
        if !self.host.scrollback() {
            return false;
        }
        let Some(pane) = self.focus() else {
            return false;
        };
        let Some(screen) = self.model.panes.get(&pane) else {
            return false;
        };
        let Some(offset) = scroll_input::page(
            key,
            modifiers,
            screen.scroll.active(),
            screen.snapshot.alt,
            screen.rows(),
        ) else {
            return false;
        };
        self.wheel.reset();
        self.scroll(pane, offset);
        true
    }

    fn wheel(
        &mut self,
        delta: f64,
        surface: bool,
        modifiers: gdk::ModifierType,
    ) -> glib::Propagation {
        if modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::META_MASK,
        ) {
            self.wheel.reset();
            return glib::Propagation::Proceed;
        }
        let Some(pane) = self.focus() else {
            return glib::Propagation::Proceed;
        };
        let Some(screen) = self.model.panes.get(&pane) else {
            return glib::Propagation::Proceed;
        };
        let alt = screen.snapshot.alt;
        if self.wheel_target != Some((pane, alt)) {
            self.wheel.reset();
            self.wheel_target = Some((pane, alt));
        }
        if (alt && (screen.modes & (1 << 8) == 0 || screen.modes & 0x1c != 0))
            || (!alt && !self.host.scrollback())
        {
            self.wheel.reset();
            return glib::Propagation::Proceed;
        }
        let height = self
            .renderer
            .as_ref()
            .map(Renderer::metrics)
            .map_or(16.0, |metrics| {
                f64::from(metrics.cell_h / self.scale.max(1.0))
            });
        let lines = self.wheel.lines(delta, surface, height);
        if lines != 0 {
            self.input.cancel_preedit();
            if alt {
                let key = if lines < 0 {
                    gdk::Key::Up
                } else {
                    gdk::Key::Down
                };
                if let Some(bytes) =
                    input::encode(key, gdk::ModifierType::empty(), screen.app_cursor())
                {
                    // Keep the fractional wheel remainder across generated arrow batches.
                    self.host.send(Message::Keys {
                        pane_id: pane,
                        bytes: bytes.repeat(lines.unsigned_abs() as usize),
                    });
                }
            } else {
                self.scroll(pane, lines);
            }
        }
        glib::Propagation::Stop
    }

    pub(super) fn update_scroll(&self) {
        let scroll = self
            .focus()
            .and_then(|pane| self.model.panes.get(&pane))
            .map(|screen| &screen.scroll);
        let active = self.host.scrollback() && scroll.is_some_and(|scroll| scroll.active());
        self.scroll_controls.set_visible(active);
        if let Some(scroll) = scroll.filter(|_| active) {
            self.scroll_label.set_text(&format!(
                "Scrollback · {} / {}",
                scroll.offset, scroll.history_len
            ));
        }
        self.scroll_bar.queue_draw();
    }
}

pub fn install(workspace: &Rc<RefCell<Workspace>>, bottom: &gtk::Button) {
    let weak = Rc::downgrade(workspace);
    bottom.connect_clicked(move |_| {
        if let Some(workspace) = weak.upgrade() {
            let mut workspace = workspace.borrow_mut();
            workspace.prepare_input();
            workspace.area.grab_focus();
        }
    });
    let area = workspace.borrow().area.clone();
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let weak = Rc::downgrade(workspace);
    scroll.connect_scroll(move |controller, _, dy| {
        let Some(workspace) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let result = workspace.borrow_mut().wheel(
            dy,
            controller.unit() == gdk::ScrollUnit::Surface,
            controller.current_event_state(),
        );
        result
    });
    let weak = Rc::downgrade(workspace);
    scroll.connect_scroll_begin(move |_| {
        if let Some(workspace) = weak.upgrade() {
            workspace.borrow_mut().wheel.reset();
        }
    });
    let weak = Rc::downgrade(workspace);
    area.connect_has_focus_notify(move |_| {
        if let Some(workspace) = weak.upgrade() {
            if let Ok(mut workspace) = workspace.try_borrow_mut() {
                workspace.wheel.reset();
            }
        }
    });
    area.add_controller(scroll);
    let weak = Rc::downgrade(workspace);
    workspace
        .borrow()
        .scroll_bar
        .set_draw_func(move |_, context, width, height| {
            let Some(workspace) = weak.upgrade() else {
                return;
            };
            let workspace = workspace.borrow();
            if !workspace.host.scrollback() {
                return;
            }
            for (pane, origin, size) in workspace.frames(width, height) {
                let Some(screen) = workspace.model.panes.get(&pane) else {
                    continue;
                };
                let scroll = &screen.scroll;
                if scroll.offset == 0 {
                    continue;
                }
                let total = f64::from(scroll.history_len) + screen.rows() as f64;
                let height = f64::from(size[1]);
                let thumb = (height * screen.rows() as f64 / total).max(8.0).min(height);
                let top = (height - thumb)
                    * (1.0 - f64::from(scroll.offset) / f64::from(scroll.history_len.max(1)));
                context.set_source_rgba(0.65, 0.7, 0.8, 0.8);
                context.rectangle(
                    f64::from(origin[0] + size[0]) - 4.0,
                    f64::from(origin[1]) + top,
                    3.0,
                    thumb,
                );
                let _ = context.fill();
            }
        });
}
