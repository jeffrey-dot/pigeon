//! GTK pointer events mapped to core cells; history never sends pointer bytes.
use super::Workspace;
use crate::{
    pointer::{self, Event},
    render::Renderer,
};
use gtk4::{self as gtk, gdk::ModifierType as M, glib, prelude::*};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub(super) struct Pointer {
    pub position: Option<(f64, f64)>,
    pub modifiers: M,
    buttons: HashMap<u32, (u32, [u16; 2], u32)>,
    last_motion: Option<(u32, [u16; 2], M)>,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            position: None,
            modifiers: M::empty(),
            buttons: HashMap::new(),
            last_motion: None,
        }
    }
}
impl Workspace {
    pub(super) fn hit_cell(&self, x: f64, y: f64) -> Option<(u32, [u16; 2])> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let metrics = self.renderer.as_ref().map(Renderer::metrics)?;
        let scale = self.scale.max(1.0);
        let point = [x * f64::from(scale), y * f64::from(scale)];
        self.frames(
            (self.area.width() as f32 * scale) as i32,
            (self.area.height() as f32 * scale) as i32,
        )
        .into_iter()
        .find_map(|(pane, origin, size)| {
            let local = [
                point[0] - f64::from(origin[0]),
                point[1] - f64::from(origin[1]),
            ];
            if local[0] < 0.0
                || local[1] < 0.0
                || local[0] >= f64::from(size[0])
                || local[1] >= f64::from(size[1])
            {
                return None;
            }
            let cell = [
                (local[0] / f64::from(metrics.cell_w)).floor() as u16,
                (local[1] / f64::from(metrics.cell_h)).floor() as u16,
            ];
            let screen = self.model.panes.get(&pane)?;
            screen
                .cell(usize::from(cell[0]), usize::from(cell[1]))
                .map(|_| (pane, cell))
        })
    }
    fn pointer_modes(&self, pane: u32) -> Option<u32> {
        (self.model.input_pane() == Some(pane)).then_some(())?;
        self.model
            .panes
            .get(&pane)
            .filter(|s| !s.scroll.active())
            .map(|s| s.modes)
    }
    pub(super) fn cancel_pointer(&mut self) {
        self.link_press.cancel();
        for (button, (pane, point, modes)) in self.pointer.buttons.drain().collect::<Vec<_>>() {
            if self.model.connected
                && self.model.panes.get(&pane).is_some_and(|screen| {
                    screen.exited.is_none()
                        && !screen.scroll.active()
                        && screen.modes & 0x3c == modes & 0x3c
                })
            {
                if let Some(bytes) =
                    pointer::encode((button - 1) as u8, Event::Release, point, M::empty(), modes)
                {
                    self.host.send(pigeon_core::api::Message::Keys {
                        pane_id: pane,
                        bytes,
                    });
                }
            }
        }
        self.pointer.last_motion = None;
    }
    fn pointer_press(&mut self, button: u32, x: f64, y: f64, modifiers: M) {
        self.click(x, y);
        self.pointer.position = Some((x, y));
        self.refresh_hover(modifiers);
        if button == 1 {
            self.link_press.begin(None);
        }
        let Some((pane, point)) = self.hit_cell(x, y) else {
            return;
        };
        if button == 1 && modifiers.contains(M::CONTROL_MASK) {
            if let Some(target) = self.link_at(pane, point) {
                self.link_press.begin(Some(target));
                return;
            }
        }
        if !(1..=3).contains(&button) {
            return;
        }
        let Some(modes) = self.pointer_modes(pane) else {
            return;
        };
        if let Some(bytes) =
            pointer::encode((button - 1) as u8, Event::Press, point, modifiers, modes)
        {
            self.send_report(pane, bytes);
            self.pointer.buttons.insert(button, (pane, point, modes));
        }
    }
    fn pointer_release(&mut self, button: u32, x: f64, y: f64, modifiers: M) {
        let hit = self.hit_cell(x, y);
        if button == 1 && self.link_press.consumed {
            let target = hit.and_then(|(pane, point)| self.link_at(pane, point));
            self.finish_link(target.as_ref(), modifiers);
            return;
        }
        let Some((pane, original, modes)) = self.pointer.buttons.remove(&button) else {
            return;
        };
        let point = hit.filter(|(p, _)| *p == pane).map_or(original, |(_, p)| p);
        if self
            .pointer_modes(pane)
            .is_some_and(|now| now & 0x3c == modes & 0x3c)
        {
            if let Some(bytes) =
                pointer::encode((button - 1) as u8, Event::Release, point, modifiers, modes)
            {
                self.send_report(pane, bytes);
            }
        }
    }
    fn pointer_motion(&mut self, x: f64, y: f64, modifiers: M) {
        if self.pointer.position != Some((x, y)) {
            self.link_press.cancel();
        }
        self.pointer.position = Some((x, y));
        self.refresh_hover(modifiers);
        let Some((pane, point)) = self.hit_cell(x, y) else {
            return;
        };
        if self.link_press.consumed || self.pointer.last_motion == Some((pane, point, modifiers)) {
            return;
        }
        self.pointer.last_motion = Some((pane, point, modifiers));
        let Some(modes) = self.pointer_modes(pane) else {
            return;
        };
        let button = self
            .pointer
            .buttons
            .iter()
            .find(|(_, (p, _, _))| *p == pane)
            .map(|(b, _)| (*b - 1) as u8);
        let event = if button.is_some() {
            Event::Drag
        } else {
            Event::Move
        };
        if let Some(bytes) = pointer::encode(button.unwrap_or(3), event, point, modifiers, modes) {
            self.send_report(pane, bytes);
        }
    }
    pub(super) fn mouse_wheel(&mut self, delta: f64, surface: bool, modifiers: M) -> bool {
        let Some((pane, point)) = self.pointer.position.and_then(|(x, y)| self.hit_cell(x, y))
        else {
            return false;
        };
        let Some(modes) = self
            .pointer_modes(pane)
            .filter(|m| m & pointer::ANY_MOUSE != 0)
        else {
            return false;
        };
        let height = self
            .renderer
            .as_ref()
            .map(Renderer::metrics)
            .map_or(16.0, |m| f64::from(m.cell_h / self.scale.max(1.0)));
        let lines = self.wheel.lines(delta, surface, height);
        if lines != 0 {
            self.input.cancel_preedit();
            self.link_press.cancel();
            if let Some(bytes) = pointer::encode(
                if lines < 0 { 64 } else { 65 },
                Event::Press,
                point,
                modifiers,
                modes,
            ) {
                self.send_report(pane, bytes.repeat(lines.unsigned_abs() as usize));
            }
        }
        true
    }
}

pub fn install(workspace: &Rc<RefCell<Workspace>>) {
    let area = workspace.borrow().area.clone();
    // Legacy controller exposes physical releases even after GtkGestureClick's drag threshold.
    let events = gtk::EventControllerLegacy::new();
    let weak = Rc::downgrade(workspace);
    events.connect_event(move |_, event| {
        let Some(work) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if let Some(button) = event.downcast_ref::<gtk::gdk::ButtonEvent>() {
            // GDK positions are surface-local; compute widget-local coordinates for the sidebar.
            let Some((x, y)) = local_position(&work.borrow().area, event) else {
                return glib::Propagation::Proceed;
            };
            if event.event_type() == gtk::gdk::EventType::ButtonPress {
                let area = work.borrow().area.clone();
                area.grab_focus();
                work.borrow_mut()
                    .pointer_press(button.button(), x, y, event.modifier_state());
            } else if event.event_type() == gtk::gdk::EventType::ButtonRelease {
                work.borrow_mut()
                    .pointer_release(button.button(), x, y, event.modifier_state());
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    area.add_controller(events);
    let motion = gtk::EventControllerMotion::new();
    let weak = Rc::downgrade(workspace);
    motion.connect_enter(move |controller, x, y| {
        if let Some(work) = weak.upgrade() {
            let modifiers = controller.current_event_state();
            dispatch_pointer(&work, move |work| work.pointer_motion(x, y, modifiers));
        }
    });
    let weak = Rc::downgrade(workspace);
    motion.connect_motion(move |controller, x, y| {
        if let Some(work) = weak.upgrade() {
            let modifiers = controller.current_event_state();
            dispatch_pointer(&work, move |work| work.pointer_motion(x, y, modifiers));
        }
    });
    let weak = Rc::downgrade(workspace);
    motion.connect_leave(move |_| {
        if let Some(work) = weak.upgrade() {
            dispatch_pointer(&work, |work| {
                work.link_press.cancel();
                work.pointer.position = None;
                work.refresh_hover(M::empty());
            });
        }
    });
    area.add_controller(motion);
}

fn dispatch_pointer(work: &Rc<RefCell<Workspace>>, action: impl FnOnce(&mut Workspace) + 'static) {
    if let Ok(mut borrowed) = work.try_borrow_mut() {
        action(&mut borrowed);
    } else {
        // Hiding a scroll overlay emits enter/leave synchronously inside screen application.
        let weak = Rc::downgrade(work);
        glib::idle_add_local_once(move || {
            if let Some(work) = weak.upgrade() {
                action(&mut work.borrow_mut());
            }
        });
    }
}
fn local_position(area: &gtk::GLArea, event: &gtk::gdk::Event) -> Option<(f64, f64)> {
    let (x, y) = event.position()?;
    let native = area.native()?.dynamic_cast::<gtk::Widget>().ok()?;
    let point = native.compute_point(area, &gtk::graphene::Point::new(x as f32, y as f32))?;
    Some((f64::from(point.x()), f64::from(point.y())))
}
