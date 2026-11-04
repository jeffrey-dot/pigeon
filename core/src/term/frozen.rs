//! A terminal that cannot be fed: it *is* a snapshot.
//!
//! This is the rebuild path the M0 contract asks for — take a `Snapshot` (from a
//! live term or off the wire), rebuild the client-side state from it, and read a
//! snapshot back. Nothing here re-derives grid state from bytes, which is exactly
//! the point: the snapshot must be complete.

use super::{history_budget::HistoryBudget, Cell, Snapshot, Term};

pub struct FrozenTerm {
    snapshot: Snapshot,
}

impl FrozenTerm {
    pub fn new(snapshot: Snapshot) -> Self {
        Self { snapshot }
    }

    fn picture(&self, history: &[Vec<Cell>]) -> Snapshot {
        Snapshot {
            cols: self.snapshot.cols,
            rows: self.snapshot.rows,
            alt: self.snapshot.alt,
            cursor: self.snapshot.cursor,
            scroll: self.snapshot.scroll,
            screen: self.snapshot.screen.clone(),
            history: history.to_vec(),
        }
    }
}

impl Term for FrozenTerm {
    fn set_palette(&mut self, _palette: Option<crate::term::Palette>) {
        // A frozen term does not parse, so it never answers colour queries.
    }

    fn feed(&mut self, _bytes: &[u8]) {
        // A frozen term is a picture, not a parser. Live deltas are ignored on
        // purpose: the host sends a fresh snapshot when the client cannot follow.
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.snapshot.cols = cols;
        self.snapshot.rows = rows;
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    fn screen_snapshot(&self) -> Snapshot {
        self.picture(&[])
    }

    fn snapshot_with_history_budget(&self, bytes: usize) -> Snapshot {
        self.picture(HistoryBudget::new(bytes).window(&self.snapshot.history, self.snapshot.cols))
    }

    fn take_damage(&mut self) -> Vec<u16> {
        Vec::new()
    }

    fn take_replies(&mut self) -> Vec<u8> {
        Vec::new()
    }

    fn title(&self) -> Option<String> {
        None
    }
}
