//! Client scroll requests and host slice ordering (docs/protocol.md §3.3).
//! History itself always stays in the core; offsets here describe only the displayed slice.

#[derive(Default)]
pub struct ScrollState {
    pub history_len: u32,
    pub offset: u32,
    pending: usize,
    returning: bool,
}

impl ScrollState {
    pub fn active(&self) -> bool {
        self.offset > 0 || self.pending > 0
    }

    pub fn request(&mut self, offset: i32) -> bool {
        if offset == 0 && (!self.active() || self.returning) {
            return false;
        }
        self.pending += 1;
        self.returning = offset == 0;
        true
    }

    /// Scroll answers are ordered, have no request id, and alone carry these two fields.
    /// Discard intermediate answers so an older slice cannot replace a newer user intent.
    pub fn reply(&mut self, history_len: u32, offset: u32) -> bool {
        if self.pending == 0 {
            return false;
        }
        self.pending -= 1;
        if self.pending != 0 {
            return false;
        }
        self.returning = false;
        self.history_len = history_len;
        self.offset = offset;
        true
    }
}

#[cfg(test)]
mod tests;
