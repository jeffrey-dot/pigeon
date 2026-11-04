//! One pane's screen, as the host describes it.
//!
//! Snapshots establish the grid and sequence; contiguous deltas patch it. The core
//! derives cell widths for both paths (`docs/protocol.md` §3.3).

use pigeon_core::api::{delta_of, snapshot_of, Message, PaneId, Snapshot};

#[cfg(test)]
mod tests;

pub struct Screen {
    pub pane_id: PaneId,
    pub snapshot: Snapshot,
    seq: u32,
    /// `term::MODE_*` bits from the message; only the app-cursor bit is used so far.
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
        Some(Self {
            pane_id,
            snapshot,
            seq,
            modes,
            exited: None,
        })
    }

    /// False requests a fresh snapshot; validation never leaves a partially patched grid.
    pub fn apply_delta(&mut self, message: &Message) -> bool {
        let Ok((pane_id, delta)) = delta_of(message) else {
            return false;
        };
        if pane_id != self.pane_id {
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
        true
    }

    pub fn cols(&self) -> usize {
        usize::from(self.snapshot.cols)
    }

    pub fn rows(&self) -> usize {
        usize::from(self.snapshot.rows)
    }

    /// DECCKM (`?1h`): unmodified arrows go out as `SS3` (protocol §3.3, `modes` bit 0).
    pub fn app_cursor(&self) -> bool {
        self.modes & 1 != 0
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<&pigeon_core::api::Cell> {
        self.snapshot.screen.get(y).and_then(|row| row.get(x))
    }
}
