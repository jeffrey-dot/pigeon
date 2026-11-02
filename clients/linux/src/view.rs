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
use pigeon_core::api::{layout_focus, layout_rects, layout_splits, Message, PaneId, Rect, Split, SplitAxis};

use crate::host::HostLink;
use crate::input;
use crate::render::{self, GridMetrics, PaneFrame, Renderer};
use crate::screen::Screen;

/// The ratio a new split starts at: half of the pane being split.
const SPLIT_RATIO: f32 = 0.5;

pub struct Workspace {
    pub area: gtk::GLArea,
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

        let focus = Some(first.pane_id);
        let workspace = Rc::new(RefCell::new(Self {
            area: area.clone(),
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

        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&workspace);
        keys.connect_key_pressed(move |_, key, _code, modifiers| match weak.upgrade() {
            Some(workspace) => workspace.borrow_mut().key(key, modifiers),
            None => glib::Propagation::Proceed,
        });
        area.add_controller(keys);

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
        self.focus
            .filter(|pane| self.panes.contains_key(pane))
            .or_else(|| self.panes.keys().copied().min())
    }

    /// Applies one host message; `false` means "attach again" for the pane it named.
    pub fn apply(&mut self, message: &Message) -> bool {
        match message {
            Message::Layout { .. } => {
                self.adopt_layout(message);
                true
            }
            Message::Snapshot { .. } => match Screen::from_message(message) {
                Some(screen) => {
                    self.panes.insert(screen.pane_id, screen);
                    self.area.queue_render();
                    true
                }
                None => false,
            },
            Message::PaneExited { pane_id, code } => {
                if let Some(screen) = self.panes.get_mut(pane_id) {
                    screen.exited = Some(*code);
                    self.area.queue_render();
                }
                true
            }
            // Not negotiated: a delta means the connection is out of step.
            Message::Delta { pane_id, .. } => {
                self.attach(*pane_id);
                true
            }
            _ => true,
        }
    }

    /// Asks the host for a pane's screen. Called for every leaf of a new layout, so a split
    /// (`Ctrl+Shift+D`) only needs the `layout` message to appear.
    fn attach(&mut self, pane_id: PaneId) {
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
        self.zoom = match message {
            Message::Layout { zoom, .. } => *zoom,
            _ => None,
        };
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
        let Some(metrics) = self.renderer.as_ref().map(Renderer::metrics) else {
            return;
        };
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_scale(scale);
        }
        self.sync_sizes(&metrics, width, height);

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
        }
    }

    fn key(&mut self, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> glib::Propagation {
        let Some(pane) = self.focus() else {
            return glib::Propagation::Proceed;
        };
        // Window chords first: they belong to the app, not to the program in the pane.
        if modifiers
            .contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK)
        {
            self.request += 1;
            let id = self.request;
            let chord = match key {
                gtk::gdk::Key::d => Some(Message::SplitPane {
                    id,
                    pane_id: pane,
                    axis: SplitAxis::Vertical,
                    ratio: SPLIT_RATIO,
                }),
                gtk::gdk::Key::e => Some(Message::SplitPane {
                    id,
                    pane_id: pane,
                    axis: SplitAxis::Horizontal,
                    ratio: SPLIT_RATIO,
                }),
                gtk::gdk::Key::w => Some(Message::ClosePane { id, pane_id: pane }),
                gtk::gdk::Key::z => Some(if self.zoom == Some(pane) {
                    Message::Unzoom { id, pane_id: pane }
                } else {
                    Message::ZoomPane { id, pane_id: pane }
                }),
                _ => None,
            };
            if let Some(message) = chord {
                self.host.send(message);
                return glib::Propagation::Stop;
            }
        }

        let app_cursor = self
            .panes
            .get(&pane)
            .map(Screen::app_cursor)
            .unwrap_or(false);
        match input::encode(key, modifiers, app_cursor) {
            Some(bytes) => {
                self.host.send(Message::Keys {
                    pane_id: pane,
                    bytes,
                });
                glib::Propagation::Stop
            }
            // Unhandled chords stay with GTK (default bindings, accelerators).
            None => glib::Propagation::Proceed,
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
                self.focus = Some(pane);
                self.host.send(Message::Focus { pane_id: pane });
                self.area.queue_render();
            }
        }
    }
}

/// Where each pane goes, in device pixels.
///
/// Split out of `Workspace` so the geometry is a pure function: the layout maths is the part
/// that is easy to get wrong, and it can be tested without a window.
pub fn place(
    layout: &[(PaneId, Rect)],
    zoom: Option<PaneId>,
    canvas: [f32; 2],
    known: &[PaneId],
) -> Vec<(PaneId, [f32; 2], [f32; 2])> {
    if let Some(zoom) = zoom.filter(|pane| known.contains(pane)) {
        return vec![(zoom, [0.0, 0.0], canvas)];
    }
    if layout.is_empty() {
        // No layout yet: the pane we attached to first owns the window.
        return known
            .iter()
            .min()
            .map(|pane| vec![(*pane, [0.0, 0.0], canvas)])
            .unwrap_or_default();
    }
    layout
        .iter()
        .filter(|(pane, _)| known.contains(pane))
        .map(|(pane, rect)| {
            (
                *pane,
                [rect.x * canvas[0], rect.y * canvas[1]],
                [rect.w * canvas[0], rect.h * canvas[1]],
            )
        })
        .collect()
}

/// Cell geometry for the window's first size, before any GL context exists.
pub fn measure(scale: f32) -> Option<GridMetrics> {
    render::measure(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn side_by_side_panes_split_the_canvas() {
        let layout = [
            (1, rect(0.0, 0.0, 0.5, 1.0)),
            (2, rect(0.5, 0.0, 0.5, 1.0)),
        ];
        let placed = place(&layout, None, [1000.0, 400.0], &[1, 2]);
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0], (1, [0.0, 0.0], [500.0, 400.0]));
        assert_eq!(placed[1], (2, [500.0, 0.0], [500.0, 400.0]));
    }

    #[test]
    fn zoom_gives_one_pane_the_whole_area() {
        let layout = [
            (1, rect(0.0, 0.0, 0.5, 1.0)),
            (2, rect(0.5, 0.0, 0.5, 1.0)),
        ];
        let placed = place(&layout, Some(2), [800.0, 600.0], &[1, 2]);
        assert_eq!(placed, vec![(2, [0.0, 0.0], [800.0, 600.0])]);
    }

    #[test]
    fn a_pane_without_a_screen_yet_is_not_drawn() {
        // The layout names a pane whose snapshot has not arrived: drawing it would paint an
        // empty rect over its sibling's half.
        let layout = [
            (1, rect(0.0, 0.0, 0.5, 1.0)),
            (2, rect(0.5, 0.0, 0.5, 1.0)),
        ];
        let placed = place(&layout, None, [100.0, 100.0], &[2]);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].0, 2);
    }

    #[test]
    fn no_layout_yet_falls_back_to_the_first_pane() {
        let placed = place(&[], None, [10.0, 20.0], &[7]);
        assert_eq!(placed, vec![(7, [0.0, 0.0], [10.0, 20.0])]);
    }
}
