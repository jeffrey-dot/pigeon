//! One pane on screen: the `GtkGLArea`, the renderer behind it, and the messages that
//! change what it draws.
//!
//! Everything here runs on the GTK main thread. The only other threads are the socket's two
//! (`host.rs`); a message crosses over as a channel send, a new frame as `queue_render`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use pigeon_core::api::{Message, PaneId};

use crate::host::HostLink;
use crate::input;
use crate::render::{self, GridMetrics, Renderer};
use crate::screen::Screen;

pub struct Pane {
    pub area: gtk::GLArea,
    screen: Screen,
    host: HostLink,
    renderer: Option<Renderer>,
    metrics: Option<GridMetrics>,
    scale: f32,
    /// The size the host was last told, so a resize storm sends one message.
    sized: (u16, u16),
    reported_failure: bool,
}

impl Pane {
    pub fn new(screen: Screen, host: HostLink, scale: f32) -> Rc<RefCell<Self>> {
        let area = gtk::GLArea::new();
        // 3.0 is the floor for instanced draws on both GLES and desktop GL as GDK hands them
        // out; the shader dialect follows whatever context we actually get (`gl::Dialect`).
        area.set_required_version(3, 0);
        area.set_has_depth_buffer(false);
        // Frames are drawn when a message says so, not on every GTK damage event.
        // GTK queues a draw on damage and on resize; a new frame from the host asks for one
        // explicitly (`queue_render` in `apply`).
        area.set_focusable(true);
        area.set_hexpand(true);
        area.set_vexpand(true);

        let pane = Rc::new(RefCell::new(Self {
            area: area.clone(),
            screen,
            host,
            renderer: None,
            metrics: None,
            scale,
            sized: (0, 0),
            reported_failure: false,
        }));

        let weak = Rc::downgrade(&pane);
        area.connect_render(move |area, _context| {
            if let Some(pane) = weak.upgrade() {
                pane.borrow_mut().draw(area);
            }
            glib::Propagation::Stop
        });

        let weak = Rc::downgrade(&pane);
        area.connect_resize(move |area, _width, _height| {
            if let Some(pane) = weak.upgrade() {
                pane.borrow_mut().resized(area);
            }
        });

        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&pane);
        keys.connect_key_pressed(move |_, key, _code, modifiers| match weak.upgrade() {
            Some(pane) => pane.borrow_mut().key(key, modifiers),
            None => glib::Propagation::Proceed,
        });
        area.add_controller(keys);

        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_PRIMARY);
        let weak = Rc::downgrade(&pane);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(pane) = weak.upgrade() {
                let pane = pane.borrow();
                pane.area.grab_focus();
                // Size policy `latest` gives the pane to whoever typed last (protocol §6).
                pane.host.send(Message::Focus {
                    pane_id: pane.screen.pane_id,
                });
            }
        });
        area.add_controller(click);

        pane
    }

    pub fn pane_id(&self) -> PaneId {
        self.screen.pane_id
    }

    /// The grid size the pane currently has, for the window's first size.
    pub fn grid(&self) -> (usize, usize) {
        (self.screen.cols(), self.screen.rows())
    }

    /// Applies a snapshot/exit; `false` means the caller should `attach` again.
    pub fn apply(&mut self, message: &Message) -> bool {
        let handled = self.screen.apply(message);
        self.area.queue_render();
        handled
    }

    fn draw(&mut self, area: &gtk::GLArea) {
        if self.renderer.is_none() {
            match Renderer::new(self.scale) {
                Ok(renderer) => {
                    self.metrics = Some(renderer.metrics());
                    self.renderer = Some(renderer);
                }
                Err(error) => {
                    if !self.reported_failure {
                        eprintln!("pigeon: no OpenGL 3.3 renderer: {error}");
                        self.reported_failure = true;
                    }
                    return;
                }
            }
        }
        self.sync_size(area);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_scale(self.scale);
            let scale = self.scale.max(1.0);
            renderer.draw(
                &self.screen,
                true,
                (area.width() as f32 * scale) as i32,
                (area.height() as f32 * scale) as i32,
            );
        }
    }

    fn resized(&mut self, area: &gtk::GLArea) {
        self.scale = area.scale_factor() as f32;
        // The framebuffer size is GTK's business; the grid follows it on the next frame.
        area.queue_render();
    }

    /// Turns the widget's pixel size into columns and rows, and tells the host once.
    fn sync_size(&mut self, area: &gtk::GLArea) {
        let scale = self.scale.max(1.0);
        let Some(metrics) = self.metrics.or_else(|| render::measure(self.scale)) else {
            return;
        };
        self.metrics = Some(metrics);
        let cols = ((area.width() as f32 * scale) / metrics.cell_w)
            .floor()
            .clamp(1.0, f32::from(u16::MAX)) as u16;
        let rows = ((area.height() as f32 * scale) / metrics.cell_h)
            .floor()
            .clamp(1.0, f32::from(u16::MAX)) as u16;
        if (cols, rows) == self.sized {
            return;
        }
        self.sized = (cols, rows);
        self.host.send(Message::ResizePane {
            pane_id: self.screen.pane_id,
            cols,
            rows,
        });
    }

    fn key(&mut self, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> glib::Propagation {
        match input::encode(key, modifiers, self.screen.app_cursor()) {
            Some(bytes) => {
                self.host.send(Message::Keys {
                    pane_id: self.screen.pane_id,
                    bytes,
                });
                glib::Propagation::Stop
            }
            // Unhandled chords stay with GTK (default bindings, accelerators).
            None => glib::Propagation::Proceed,
        }
    }
}
