//! Divider gestures consume their whole pointer sequence; only host replies change layout.
use super::Workspace;
use crate::divider;
use gtk4::prelude::*;
use pigeon_core::api::{Axis, Message, Split};
use std::time::{Duration, Instant};

pub(super) struct Drag {
    split: Split,
    generation: u64,
    last_sent: Instant,
    last_ratio: f32,
}

impl Workspace {
    pub(super) fn dragging_divider(&self) -> Option<Axis> {
        self.divider_drag.as_ref().map(|drag| drag.split.axis)
    }
    fn divider_canvas(&self) -> [f32; 2] {
        [self.area.width() as f32, self.area.height() as f32]
    }

    pub(super) fn divider_at(&self, x: f64, y: f64) -> Option<Split> {
        if !self.model.connected || !self.host.connected() || self.focus().is_none() {
            return None;
        }
        divider::hit(
            &self.model.splits,
            self.model.zoom.is_some(),
            self.divider_canvas(),
            [x as f32, y as f32],
        )
    }

    pub(super) fn begin_divider(&mut self, x: f64, y: f64) -> bool {
        let Some(split) = self.divider_at(x, y) else {
            return false;
        };
        self.cancel_input();
        self.close_search(true);
        self.input.cancel_preedit();
        self.divider_drag = Some(Drag {
            split,
            generation: self.model.selection_generation,
            last_sent: Instant::now() - Duration::from_millis(16),
            last_ratio: split.ratio,
        });
        self.divider_cursor(split.axis);
        true
    }

    pub(super) fn move_divider(&mut self, x: f64, y: f64, finished: bool) -> bool {
        let Some(drag) = self.divider_drag.as_ref() else {
            return false;
        };
        let valid = self.model.connected
            && self.host.connected()
            && self.model.zoom.is_none()
            && self.model.selection_generation == drag.generation
            && self.model.splits.iter().any(|s| {
                s.id == drag.split.id && s.axis == drag.split.axis && s.rect == drag.split.rect
            });
        if valid {
            let split = drag.split;
            if let Some(ratio) = divider::ratio(&split, self.divider_canvas(), [x as f32, y as f32])
            {
                if (finished || drag.last_sent.elapsed() >= Duration::from_millis(16))
                    && (ratio - drag.last_ratio).abs() > 0.0001
                {
                    let id = self.model.request_id();
                    self.host.send(Message::ResizeSplit {
                        id,
                        split_id: split.id,
                        ratio,
                    });
                    if let Some(drag) = self.divider_drag.as_mut() {
                        drag.last_sent = Instant::now();
                        drag.last_ratio = ratio;
                    }
                }
            }
            self.divider_cursor(split.axis);
        }
        if finished {
            self.divider_drag = None;
            self.refresh_hover(self.pointer.modifiers);
        }
        true
    }

    pub(super) fn divider_cursor(&self, axis: Axis) {
        self.area.set_cursor_from_name(Some(match axis {
            Axis::Vertical => "col-resize",
            Axis::Horizontal => "row-resize",
        }));
    }
}
