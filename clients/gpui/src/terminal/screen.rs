//! One pane's screen, as the host describes it.
//!
//! Snapshots establish the grid and sequence; contiguous deltas patch it. The core derives
//! cell widths for both paths (`docs/protocol.md` §3.3). Adapted from the Linux client's
//! `screen.rs`, trimmed to snapshot+delta (scrollback, search and selection arrive in later
//! stages). The sequence/validation discipline is kept identical so behaviour matches.

use pigeon_core::api::{delta_of, snapshot_of, Message, PaneId, Snapshot};

pub struct Screen {
    pub pane_id: PaneId,
    pub snapshot: Snapshot,
    seq: u32,
    /// Bumped on every accepted change, so the view knows when to repaint.
    pub revision: u64,
    /// Mode bits from docs/protocol.md §3.3.
    pub modes: u32,
}

impl Screen {
    /// The pane and screen carried by a `snapshot`; `None` for any other message. This stage
    /// ignores scrollback, so a scrolled snapshot (`offset`/`history_len`) is treated as the
    /// live screen it already contains.
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
            revision: 1,
            modes,
        })
    }

    /// A newer snapshot replaces the grid; a stale one is ignored.
    pub fn apply_snapshot(&mut self, message: &Message) -> bool {
        let Some(next) = Self::from_message(message) else {
            return false;
        };
        if next.pane_id != self.pane_id {
            return false;
        }
        if next.seq < self.seq {
            return true;
        }
        self.snapshot = next.snapshot;
        self.seq = next.seq;
        self.modes = next.modes;
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// False requests a fresh snapshot; validation never leaves a partially patched grid.
    pub fn apply_delta(&mut self, message: &Message) -> bool {
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
    pub fn rows(&self) -> usize {
        usize::from(self.snapshot.rows)
    }
}
