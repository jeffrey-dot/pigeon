//! [`Outbox`]: what a pane has produced but the client has not taken yet.
//!
//! The terminal hands replies and damage out by draining them. A C caller with a
//! buffer that is too small must not lose them, so the core keeps them here until
//! the caller says how much it consumed.

/// Pending replies and dirty rows of one pane.
#[derive(Debug, Default)]
pub(crate) struct Outbox {
    replies: Vec<u8>,
    /// Sorted and deduplicated: a row dirtied twice is redrawn once.
    dirty: Vec<u16>,
}

impl Outbox {
    pub(crate) fn push_replies(&mut self, bytes: Vec<u8>) {
        if self.replies.is_empty() {
            self.replies = bytes;
        } else {
            self.replies.extend_from_slice(&bytes);
        }
    }

    pub(crate) fn push_dirty(&mut self, rows: Vec<u16>) {
        if rows.is_empty() {
            return;
        }
        self.dirty.extend(rows);
        self.dirty.sort_unstable();
        self.dirty.dedup();
    }

    /// A resize leaves rows past the new bottom; they no longer exist.
    pub(crate) fn clip_rows(&mut self, rows: u16) {
        self.dirty.retain(|row| *row < rows);
    }

    pub(crate) fn replies(&self) -> &[u8] {
        &self.replies
    }

    pub(crate) fn dirty(&self) -> &[u16] {
        &self.dirty
    }

    pub(crate) fn consume_replies(&mut self, count: usize) {
        let count = count.min(self.replies.len());
        self.replies.drain(..count);
    }

    pub(crate) fn consume_dirty(&mut self, count: usize) {
        let count = count.min(self.dirty.len());
        self.dirty.drain(..count);
    }

    pub(crate) fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub(crate) fn take_dirty(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.dirty)
    }
}
