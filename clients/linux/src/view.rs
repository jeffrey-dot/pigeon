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
use pigeon_core::api::{Message, PaneId};

use crate::host::HostLink;
use crate::input_context::InputContext;
use crate::placement::place;
use crate::render::{self, GridMetrics, PaneFrame, Renderer};
use crate::session_model::SessionModel;
use crate::sidebar::SidebarModel;

mod divider;
mod input;
mod links;
mod pointer;
mod protocol;
mod recovery;
mod scroll;
mod search;

pub struct Workspace {
    pub area: gtk::GLArea,
    pub root: gtk::Overlay,
    pub input: Rc<InputContext>,
    host: HostLink,
    pub model: SessionModel,
    pub status: SidebarModel,
    pub sizes: HashMap<PaneId, (String, u8)>,
    presented: HashMap<PaneId, u64>,
    scale: f32,
    renderer: Option<Renderer>,
    palette: render::Palette,
    /// The size each pane was last told, so a resize storm sends one message per change.
    sized: HashMap<PaneId, (u16, u16)>,
    reported_failure: bool,
    wheel: crate::scroll_input::Wheel,
    wheel_target: Option<(PaneId, bool)>,
    scroll_label: gtk::Label,
    scroll_controls: gtk::Box,
    scroll_bar: gtk::DrawingArea,
    terminal_focus: Option<PaneId>,
    pointer: pointer::Pointer,
    link_press: crate::links::LinkPress,
    pub search: Option<crate::search::Search>,
    pub search_supported: bool,
    pub disconnected_reason: String,
    pub connection_generation: u64,
    pub host_instance: Option<String>,
    pub host_identity: String,
    pub recovery_notice: String,
    divider_drag: Option<divider::Drag>,
}

impl Workspace {
    pub fn new(hello: &Message, host: HostLink, scale: f32) -> Rc<RefCell<Self>> {
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
        let mut status = SidebarModel::default();
        if let Message::HelloAck {
            host_id,
            instance,
            features,
            ..
        } = hello
        {
            status.begin_connection(
                host_id,
                instance.as_deref(),
                features.iter().any(|f| f == "status_snapshot"),
            );
        }
        let model = SessionModel::new(host.scrollback());
        let workspace = Rc::new(RefCell::new(Self {
            area: area.clone(),
            root,
            input,
            host,
            model,
            status,
            sizes: HashMap::new(),
            presented: Default::default(),
            scale,
            renderer: None,
            palette: if crate::theme::dark() {
                render::Palette::dark()
            } else {
                render::Palette::light()
            },
            sized: HashMap::new(),
            reported_failure: false,
            wheel: Default::default(),
            wheel_target: None,
            scroll_label,
            scroll_controls,
            scroll_bar,
            terminal_focus: None,
            pointer: Default::default(),
            link_press: Default::default(),
            search: None,
            disconnected_reason: String::new(),
            connection_generation: 1,
            host_instance: match hello {
                Message::HelloAck { instance, .. } => instance.clone(),
                _ => None,
            },
            host_identity: match hello {
                Message::HelloAck { host_id, .. } => host_id.clone(),
                _ => String::new(),
            },
            recovery_notice: String::new(),
            divider_drag: None,
            search_supported: pigeon_core::api::offers_feature(hello, "text_query")
                .unwrap_or(false)
                && pigeon_core::api::offers_feature(hello, "text_viewport").unwrap_or(false),
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

        pointer::install(&workspace);

        workspace
    }

    /// Only the selected session's established layout can receive input.
    pub fn focus(&self) -> Option<PaneId> {
        self.model.focused_pane()
    }

    /// The panes to draw and where, in device pixels. A zoomed pane takes the whole area.
    fn frames(&self, width: i32, height: i32) -> Vec<(PaneId, [f32; 2], [f32; 2])> {
        if self.model.layout.is_empty() {
            return Vec::new();
        }
        let known: Vec<PaneId> = self.model.panes.keys().copied().collect();
        place(
            &self.model.layout,
            self.model.zoom,
            [width as f32, height as f32],
            &known,
        )
    }

    fn draw(&mut self, area: &gtk::GLArea) {
        if self
            .renderer
            .as_ref()
            .is_some_and(|renderer| !renderer.owns_current_context())
        {
            self.renderer = None;
        }
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
            renderer.set_palette(self.palette);
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
                self.model.panes.get(pane).map(|screen| PaneFrame {
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
            self.presented
                .extend(ordered.iter().filter_map(|(pane, _, _)| {
                    self.model
                        .panes
                        .get(pane)
                        .map(|screen| (*pane, screen.revision))
                }));
        }
        self.refresh_hover(self.pointer.modifiers);
    }

    pub fn set_appearance(&mut self, dark: bool) {
        self.palette = if dark {
            render::Palette::dark()
        } else {
            render::Palette::light()
        };
        self.host.send(Message::Appearance {
            fg: Some(self.palette.foreground),
            bg: Some(self.palette.background),
            ansi: self.palette.ansi.to_vec(),
        });
        self.area.queue_render();
    }

    /// One line per split, in device pixels: without it two panes of the same background
    /// look like one wide one.
    fn dividers(&self, width: i32, height: i32) -> Vec<[f32; 4]> {
        if self.model.zoom.is_some() {
            return Vec::new();
        }
        let canvas = [width as f32, height as f32];
        self.model
            .splits
            .iter()
            .map(|split| crate::divider::line(split, canvas, self.scale.max(1.0).round()))
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
            if self.model.focus != Some(pane) {
                self.close_search(false);
                self.cancel_input();
                self.wheel.reset();
                self.model.focus = Some(pane);
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
