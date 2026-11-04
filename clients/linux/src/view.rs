//! The workspace: one `GtkGLArea` for the whole window, drawing every pane of the session at
//! the rect the host's `layout` message gives it.
//!
//! One surface rather than one GLArea per pane: a frame is a single clear plus N batches, the
//! layout comes from the host anyway, and hit-testing for focus is a rect test instead of GTK
//! child coordinates. The macOS client draws one Metal view per pane; the split of
//! responsibilities is the same, the number of contexts is not.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use pigeon_core::api::{layout_focus, layout_rects, layout_splits, Message, PaneId, Rect, Split};

use crate::host::HostLink;
use crate::input_context::InputContext;
use crate::placement::place;
use crate::render::{self, GridMetrics, PaneFrame, Renderer};
use crate::screen::Screen;

mod input;
mod scroll;

pub struct Workspace {
    pub area: gtk::GLArea,
    pub root: gtk::Overlay,
    pub input: Rc<InputContext>,
    host: HostLink,
    panes: HashMap<PaneId, Screen>,
    /// `layout` came through normalized (0…1) and in tree order.
    layout: Vec<(PaneId, Rect)>,
    /// Dividers between panes, for hit-testing later and for the visible seam now.
    splits: Vec<Split>,
    zoom: Option<PaneId>,
    focus: Option<PaneId>,
    scale: f32,
    renderer: Option<Renderer>,
    /// The size each pane was last told, so a resize storm sends one message per change.
    sized: HashMap<PaneId, (u16, u16)>,
    request: u32,
    reported_failure: bool,
    wheel: crate::scroll_input::Wheel,
    wheel_target: Option<(PaneId, bool)>,
    scroll_label: gtk::Label,
    scroll_controls: gtk::Box,
    scroll_bar: gtk::DrawingArea,
}

impl Workspace {
    pub fn new(first: Screen, host: HostLink, scale: f32) -> Rc<RefCell<Self>> {
        let area = gtk::GLArea::new();
        // 3.0 is the floor for instanced draws on both GLES and desktop GL as GDK hands them
        // out; the shader dialect follows whatever context we actually get (`gl::Dialect`).
        area.set_required_version(3, 0);
        area.set_has_depth_buffer(false);
        // GTK queues a draw on damage and on resize; a new frame from the host asks for one
        // explicitly (`queue_render` in `apply`).
        area.set_focusable(true);
        area.set_hexpand(true);
        area.set_vexpand(true);

        let root = gtk::Overlay::new();
        root.set_child(Some(&area));
        let input = InputContext::new(&area, &root);
        let scroll_label = gtk::Label::new(None);
        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrColor::new_foreground(
            0xeeee, 0xeeee, 0xeeee,
        ));
        attrs.insert(gtk::pango::AttrColor::new_background(
            0x2222, 0x2222, 0x2222,
        ));
        scroll_label.set_attributes(Some(&attrs));
        let scroll_controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        scroll_controls.set_halign(gtk::Align::End);
        scroll_controls.set_valign(gtk::Align::Start);
        scroll_controls.set_visible(false);
        scroll_controls.append(&scroll_label);
        let bottom = gtk::Button::with_label("Return to live");
        bottom.set_focusable(false);
        scroll_controls.append(&bottom);
        let scroll_bar = gtk::DrawingArea::new();
        scroll_bar.set_can_target(false);
        root.add_overlay(&scroll_bar);
        root.add_overlay(&scroll_controls);
        input.set_target(Some(first.pane_id));
        let focus = Some(first.pane_id);
        let workspace = Rc::new(RefCell::new(Self {
            area: area.clone(),
            root,
            input,
            host,
            panes: HashMap::from([(first.pane_id, first)]),
            layout: Vec::new(),
            splits: Vec::new(),
            zoom: None,
            focus,
            scale,
            renderer: None,
            sized: HashMap::new(),
            request: 10,
            reported_failure: false,
            wheel: Default::default(),
            wheel_target: None,
            scroll_label,
            scroll_controls,
            scroll_bar,
        }));

        let weak = Rc::downgrade(&workspace);
        area.connect_render(move |area, _context| {
            if let Some(workspace) = weak.upgrade() {
                workspace.borrow_mut().draw(area);
            }
            glib::Propagation::Stop
        });

        let weak = Rc::downgrade(&workspace);
        area.connect_resize(move |area, _width, _height| {
            if let Some(workspace) = weak.upgrade() {
                workspace.borrow_mut().scale = area.scale_factor() as f32;
                area.queue_render();
            }
        });

        crate::input_context::install(&workspace);
        scroll::install(&workspace, &bottom);

        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_PRIMARY);
        let weak = Rc::downgrade(&workspace);
        click.connect_pressed(move |_, _, x, y| {
            if let Some(workspace) = weak.upgrade() {
                workspace.borrow_mut().click(x, y);
            }
        });
        area.add_controller(click);

        workspace
    }

    /// The pane the keyboard goes to.
    pub fn focus(&self) -> Option<PaneId> {
        match self.focus {
            Some(pane) => self.panes.contains_key(&pane).then_some(pane),
            None => self.panes.keys().copied().min(),
        }
    }

    /// Applies one host message; `false` means "attach again" for the pane it named.
    pub fn apply(&mut self, message: &Message) -> bool {
        let applied = match message {
            Message::Layout { .. } => {
                self.adopt_layout(message);
                true
            }
            Message::Snapshot { pane_id, .. } => {
                let applied = if let Some(screen) = self.panes.get_mut(pane_id) {
                    screen.apply_snapshot(message)
                } else if let Some(screen) = Screen::from_message(message) {
                    // A late reply from a closed pane must not resurrect its input target.
                    if self.layout.iter().any(|(id, _)| id == pane_id) {
                        self.panes.insert(*pane_id, screen);
                    }
                    true
                } else {
                    false
                };
                self.area.queue_render();
                applied
            }
            Message::SizeState {
                pane_id,
                cols,
                rows,
                ..
            } => {
                if self.panes.get(pane_id).is_some_and(|screen| {
                    screen.cols() != usize::from(*cols) || screen.rows() != usize::from(*rows)
                }) {
                    self.return_live(*pane_id);
                }
                true
            }
            Message::PaneExited { pane_id, code } => {
                if let Some(screen) = self.panes.get_mut(pane_id) {
                    screen.exited = Some(*code);
                    self.area.queue_render();
                }
                true
            }
            Message::Delta { pane_id, .. } => {
                let applied = self
                    .panes
                    .get_mut(pane_id)
                    .is_some_and(|screen| screen.apply_delta(message));
                if applied {
                    self.area.queue_render();
                } else {
                    self.attach(*pane_id);
                }
                true
            }
            _ => true,
        };
        self.update_input();
        self.update_scroll();
        applied
    }

    /// Asks the host for a pane's screen. Called for every leaf of a new layout, so a split
    /// (`Ctrl+Shift+D`) only needs the `layout` message to appear.
    fn attach(&mut self, pane_id: PaneId) {
        self.return_live(pane_id);
        self.request += 1;
        self.host.send(Message::Attach {
            id: self.request,
            pane_id,
            size_policy: None,
            history: Some(false),
            since: None,
        });
    }

    fn adopt_layout(&mut self, message: &Message) {
        let old_focus = self.focus;
        if let Ok(rects) = layout_rects(message) {
            self.layout = rects;
        }
        if let Ok(splits) = layout_splits(message) {
            self.splits = splits;
        }
        if let Ok(Some(focus)) = layout_focus(message) {
            self.focus = Some(focus);
        } else if self.focus.is_none() {
            self.focus = self.layout.first().map(|(pane, _)| *pane);
        }
        if old_focus != self.focus {
            self.wheel.reset();
        }
        self.zoom = match message {
            Message::Layout { zoom, .. } => *zoom,
            _ => None,
        };
        self.panes
            .retain(|pane, _| self.layout.iter().any(|(id, _)| id == pane));
        for (pane_id, _) in self.layout.clone() {
            if !self.panes.contains_key(&pane_id) {
                self.attach(pane_id);
            }
        }
        self.area.queue_render();
    }

    /// The panes to draw and where, in device pixels. A zoomed pane takes the whole area.
    fn frames(&self, width: i32, height: i32) -> Vec<(PaneId, [f32; 2], [f32; 2])> {
        let known: Vec<PaneId> = self.panes.keys().copied().collect();
        place(
            &self.layout,
            self.zoom,
            [width as f32, height as f32],
            &known,
        )
    }

    fn draw(&mut self, area: &gtk::GLArea) {
        if self.renderer.is_none() {
            match Renderer::new(self.scale) {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(error) => {
                    if !self.reported_failure {
                        eprintln!("pigeon: no OpenGL 3.0 renderer: {error}");
                        self.reported_failure = true;
                    }
                    return;
                }
            }
        }
        let scale = self.scale.max(1.0);
        let width = (area.width() as f32 * scale) as i32;
        let height = (area.height() as f32 * scale) as i32;
        if width <= 0 || height <= 0 {
            return;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_scale(scale);
        }
        let Some(metrics) = self.renderer.as_ref().map(Renderer::metrics) else {
            return;
        };
        self.sync_sizes(&metrics, width, height);
        self.update_input();
        self.update_scroll();

        let ordered = self.frames(width, height);
        let focused = self.focus();
        let panes: Vec<PaneFrame> = ordered
            .iter()
            .filter_map(|(pane, origin, size)| {
                self.panes.get(pane).map(|screen| PaneFrame {
                    origin: *origin,
                    size: *size,
                    screen,
                    focused: Some(*pane) == focused,
                })
            })
            .collect();
        let dividers = self.dividers(width, height);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.draw(&panes, &dividers, width, height);
        }
    }

    /// One line per split, in device pixels: without it two panes of the same background
    /// look like one wide one.
    fn dividers(&self, width: i32, height: i32) -> Vec<[f32; 4]> {
        if self.zoom.is_some() {
            return Vec::new();
        }
        let canvas = [width as f32, height as f32];
        self.splits
            .iter()
            .map(|split| {
                let x = split.rect.x * canvas[0];
                let y = split.rect.y * canvas[1];
                let w = split.rect.w * canvas[0];
                let h = split.rect.h * canvas[1];
                let line = (self.scale.max(1.0)).round();
                match split.axis {
                    pigeon_core::api::Axis::Vertical => [x - line / 2.0, y, line, h],
                    pigeon_core::api::Axis::Horizontal => [x, y - line / 2.0, w, line],
                }
            })
            .collect()
    }

    /// Tells the host, per pane, the grid size that fits its rect.
    fn sync_sizes(&mut self, metrics: &GridMetrics, width: i32, height: i32) {
        for (pane, _, [w, h]) in self.frames(width, height) {
            let cols = (w / metrics.cell_w).floor().clamp(1.0, f32::from(u16::MAX)) as u16;
            let rows = (h / metrics.cell_h).floor().clamp(1.0, f32::from(u16::MAX)) as u16;
            if self.sized.get(&pane) == Some(&(cols, rows)) {
                continue;
            }
            self.sized.insert(pane, (cols, rows));
            self.host.send(Message::ResizePane {
                pane_id: pane,
                cols,
                rows,
            });
            self.return_live(pane);
        }
    }

    /// Focus follows the click, and the host is told so `latest` sizing follows it too.
    fn click(&mut self, x: f64, y: f64) {
        self.area.grab_focus();
        let scale = f64::from(self.scale.max(1.0));
        let point = [x * scale, y * scale];
        let width = (self.area.width() as f32 * self.scale.max(1.0)) as i32;
        let height = (self.area.height() as f32 * self.scale.max(1.0)) as i32;
        let hit = self
            .frames(width, height)
            .into_iter()
            .find(|(_, origin, size)| {
                point[0] >= f64::from(origin[0])
                    && point[0] < f64::from(origin[0] + size[0])
                    && point[1] >= f64::from(origin[1])
                    && point[1] < f64::from(origin[1] + size[1])
            })
            .map(|(pane, _, _)| pane);
        if let Some(pane) = hit {
            if self.focus != Some(pane) {
                self.wheel.reset();
                self.focus = Some(pane);
                self.host.send(Message::Focus { pane_id: pane });
                self.area.queue_render();
            }
        }
        self.update_input();
        self.update_scroll();
    }
}

/// Cell geometry for the window's first size, before any GL context exists.
pub fn measure(scale: f32) -> Option<GridMetrics> {
    render::measure(scale)
}
