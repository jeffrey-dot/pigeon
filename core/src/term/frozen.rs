//! A terminal that cannot be fed: it *is* a snapshot.
//!
//! This is the rebuild path the M0 contract asks for — take a `Snapshot` (from a
//! live term or off the wire), rebuild the client-side state from it, and read a
//! snapshot back. Nothing here re-derives grid state from bytes, which is exactly
//! the point: the snapshot must be complete.

use super::{Snapshot, Term};

pub struct FrozenTerm {
    snapshot: Snapshot,
}

impl FrozenTerm {
    pub fn new(snapshot: Snapshot) -> Self {
        Self { snapshot }
    }
}

impl Term for FrozenTerm {
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

    fn take_damage(&mut self) -> Vec<u16> {
        Vec::new()
    }

    fn take_replies(&mut self) -> Vec<u8> {
        Vec::new()
    }
}
