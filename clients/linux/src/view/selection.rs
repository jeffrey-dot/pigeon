//! Pointer selection over the currently visible immutable grid.
use super::Workspace;
use crate::selection::Drag;
use gtk4::{gdk::ModifierType as M, prelude::*};
use pigeon_core::api::{self, GridPoint, GridSelection, SelectionUnit};

impl Workspace {
    pub(super) fn start_selection(&mut self, pane: u32, point: [u16; 2], modifiers: M) -> bool {
        let Some(screen) = self.model.panes.get(&pane) else {
            return false;
        };
        if screen.scroll.active() && screen.scroll.offset == 0 {
            return false;
        }
        if self.presented.get(&pane) != Some(&screen.revision) {
            return false;
        }
        let point = GridPoint {
            row: point[1],
            col: point[0],
        };
        let unit = self.selection_clicks.press(pane, point);
        let mut drag = Drag {
            pane,
            revision: screen.revision,
            request: GridSelection {
                anchor: point,
                head: point,
                unit,
                rectangle: modifiers.contains(M::ALT_MASK),
            },
            active: false,
        };
        let selection = drag.update(screen.picture(), point);
        if let Some(screen) = self.model.panes.get_mut(&pane) {
            screen.selection = selection;
        }
        self.selection_drag = Some(drag);
        self.area.queue_render();
        true
    }

    pub(super) fn move_selection(&mut self, x: f64, y: f64, finished: bool) -> bool {
        let Some(drag) = self.selection_drag.as_ref() else {
            return false;
        };
        let pane = drag.pane;
        let point = self
            .hit_cell(x, y)
            .filter(|(id, _)| *id == pane)
            .map(|(_, p)| GridPoint {
                row: p[1],
                col: p[0],
            });
        if let (Some(point), Some(screen), Some(drag)) = (
            point,
            self.model.panes.get_mut(&pane),
            self.selection_drag.as_mut(),
        ) {
            if screen.revision == drag.revision {
                screen.selection = drag.update(screen.picture(), point);
            }
        }
        if finished {
            self.selection_drag = None;
        }
        self.area.queue_render();
        true
    }

    pub fn select_visible(&mut self) {
        let Some(screen) = self
            .focus()
            .and_then(|pane| self.model.panes.get_mut(&pane))
        else {
            return;
        };
        if screen.rows() == 0 || screen.cols() == 0 {
            return;
        }
        screen.selection = api::select_grid(
            screen.picture(),
            &GridSelection {
                anchor: GridPoint { row: 0, col: 0 },
                head: GridPoint {
                    row: screen.rows() as u16 - 1,
                    col: screen.cols() as u16 - 1,
                },
                unit: SelectionUnit::Cell,
                rectangle: false,
            },
        )
        .ok();
        self.area.queue_render();
    }
    pub(super) fn clear_selection(&mut self) {
        self.selection_drag = None;
        if let Some(pane) = self.focus() {
            if let Some(screen) = self.model.panes.get_mut(&pane) {
                screen.selection = None;
            }
        }
        self.area.queue_render();
    }
}
