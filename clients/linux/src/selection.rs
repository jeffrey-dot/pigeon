//! Client gesture state only. Core decides selected cells, word boundaries and copied text.
use pigeon_core::api::{self, GridPoint, GridSelection, Selection, SelectionUnit, Snapshot};
use std::time::{Duration, Instant};

pub struct Drag {
    pub pane: u32,
    pub revision: u64,
    pub request: GridSelection,
    pub active: bool,
}
#[derive(Default)]
pub struct Clicks {
    last: Option<(u32, GridPoint, Instant, u8)>,
}
impl Clicks {
    pub fn press(&mut self, pane: u32, point: GridPoint) -> SelectionUnit {
        let now = Instant::now();
        let count = self
            .last
            .filter(|(p, at, time, _)| {
                *p == pane && *at == point && now.duration_since(*time) < Duration::from_millis(400)
            })
            .map_or(1, |(_, _, _, n)| if n >= 3 { 1 } else { n + 1 });
        self.last = Some((pane, point, now, count));
        match count {
            2 => SelectionUnit::Word,
            3 => SelectionUnit::Line,
            _ => SelectionUnit::Cell,
        }
    }
}
impl Drag {
    pub fn update(&mut self, picture: &Snapshot, point: GridPoint) -> Option<Selection> {
        self.active |= self.request.head != point || self.request.unit != SelectionUnit::Cell;
        self.request.head = point;
        self.active
            .then(|| api::select_grid(picture, &self.request).ok())
            .flatten()
    }
}
