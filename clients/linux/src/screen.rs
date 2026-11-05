//! One pane's screen, as the host describes it.
//!
//! Snapshots establish the grid and sequence; contiguous deltas patch it. The core
//! derives cell widths for both paths (`docs/protocol.md` §3.3).

use crate::scroll::ScrollState;

use pigeon_core::api::{delta_of, snapshot_of, Message, PaneId, Snapshot};

#[cfg(test)]
mod tests;

pub struct Screen {
    pub pane_id: PaneId,
    pub snapshot: Snapshot,
    seq: u32,
    pub revision: u64,
    pub search_picture: Option<Snapshot>,
    pub search_highlight: Vec<(usize, usize, usize)>,
    pub search_returning: bool,
    pub selection: Option<pigeon_core::api::Selection>,
    pub scroll: ScrollState,
    /// Mode bits from docs/protocol.md §3.3.
    pub modes: u32,
    /// Set once the program in the pane exited; the screen stays as it was.
    pub exited: Option<i32>,
}

impl Screen {
    /// The pane and screen carried by a `snapshot`; `None` for any other message.
    pub fn from_message(message: &Message) -> Option<Self> {
        let (pane_id, snapshot) = snapshot_of(message).ok()?;
        let (seq, modes) = match message {
            Message::Snapshot { seq, modes, .. } => (*seq, modes.unwrap_or(0)),
            _ => return None,
        };
        if let Message::Snapshot {
            history_len,
            offset,
            ..
        } = message
        {
            if history_len.is_some() != offset.is_some() {
                return None;
            }
        }
        let mut scroll = ScrollState::default();
        if let Message::Snapshot {
            history_len: Some(total),
            offset: Some(offset),
            ..
        } = message
        {
            if offset > total || (snapshot.alt && *offset != 0) {
                return None;
            }
            scroll.history_len = *total;
            scroll.offset = *offset;
        }
        Some(Self {
            pane_id,
            snapshot,
            seq,
            revision: 1,
            search_picture: None,
            search_highlight: vec![],
            search_returning: false,
            selection: None,
            scroll,
            modes,
            exited: None,
        })
    }

    /// A live snapshot cannot overwrite a pending or displayed history slice.
    pub fn apply_snapshot(&mut self, message: &Message) -> bool {
        let Some(next) = Self::from_message(message) else {
            return false;
        };
        if next.pane_id != self.pane_id {
            return false;
        }
        match message {
            Message::Snapshot {
                history_len: Some(total),
                offset: Some(offset),
                ..
            } => {
                if !self.scroll.reply(*total, *offset) {
                    return true;
                }
            }
            _ if self.scroll.active() || next.seq < self.seq => return true,
            _ => {}
        }
        self.snapshot = next.snapshot;
        self.selection = None;
        self.seq = next.seq;
        self.modes = next.modes;
        if self.search_returning && !self.scroll.active() {
            self.search_picture = None;
            self.search_highlight.clear();
            self.search_returning = false;
        }
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// False requests a fresh snapshot; validation never leaves a partially patched grid.
    pub fn apply_delta(&mut self, message: &Message) -> bool {
        if self.scroll.active() {
            return true;
        }
        let Ok(delta) = delta_of(message) else {
            return false;
        };
        if delta.pane_id != self.pane_id {
            return false;
        }
        if delta.seq <= self.seq {
            return true;
        }
        if self.seq.checked_add(1) != Some(delta.seq)
            || delta
                .rows
                .iter()
                .any(|(y, cells)| usize::from(*y) >= self.rows() || cells.len() != self.cols())
        {
            return false;
        }
        for (y, cells) in delta.rows {
            self.snapshot.screen[usize::from(y)] = cells;
        }
        self.selection = None;
        if let Some(cursor) = delta.cursor {
            self.snapshot.cursor = cursor;
        }
        if let Some(alt) = delta.alt {
            self.snapshot.alt = alt;
        }
        if let Some(modes) = delta.modes {
            self.modes = modes;
        }
        self.seq = delta.seq;
        self.revision = self.revision.saturating_add(1);
        true
    }

    pub fn cols(&self) -> usize {
        usize::from(self.snapshot.cols)
    }
    pub fn resume_seq(&self) -> Option<u32> {
        (!self.scroll.active() && self.search_picture.is_none() && !self.search_returning)
            .then_some(self.seq)
    }

    pub fn picture(&self) -> &Snapshot {
        self.search_picture.as_ref().unwrap_or(&self.snapshot)
    }

    pub fn rows(&self) -> usize {
        usize::from(self.snapshot.rows)
    }

    /// DECCKM (`?1h`): unmodified arrows go out as `SS3` (protocol §3.3, `modes` bit 0).
    pub fn app_cursor(&self) -> bool {
        self.modes & 1 != 0
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<&pigeon_core::api::Cell> {
        self.picture().screen.get(y).and_then(|row| row.get(x))
    }

    /// Only the visible live or scrolled screen; other history rows and panes are excluded.
    pub fn visible_text(&self) -> String {
        let picture = self.picture();
        if picture.cols == 0 || picture.rows == 0 {
            return String::new();
        }
        pigeon_core::api::select_grid(
            picture,
            &pigeon_core::api::GridSelection {
                anchor: pigeon_core::api::GridPoint { row: 0, col: 0 },
                head: pigeon_core::api::GridPoint {
                    row: picture.rows - 1,
                    col: picture.cols - 1,
                },
                unit: pigeon_core::api::SelectionUnit::Cell,
                rectangle: false,
            },
        )
        .map(|selected| selected.text)
        .unwrap_or_default()
    }
}
