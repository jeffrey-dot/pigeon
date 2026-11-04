//! History export accounting shared by live conversion and frozen snapshots.

use super::Cell;

/// Estimated wire bytes, not an allocator/RSS or whole-frame limit.
pub(crate) struct HistoryBudget {
    remaining: usize,
}

impl HistoryBudget {
    pub(crate) fn new(bytes: usize) -> Self {
        Self { remaining: bytes }
    }

    /// Charge a whole row before allocating its cells or text. Stop at the first row
    /// that does not fit: skipping it would break the contiguous newest history suffix.
    pub(crate) fn take_row(
        &mut self,
        cols: u16,
        text_lengths: impl IntoIterator<Item = usize>,
    ) -> bool {
        let Some(mut remaining) = self.remaining.checked_sub(usize::from(cols) * 20 + 16) else {
            return false;
        };
        for len in text_lengths {
            let Some(rest) = len
                .checked_add(32)
                .and_then(|bytes| remaining.checked_sub(bytes))
            else {
                return false;
            };
            remaining = rest;
        }
        self.remaining = remaining;
        true
    }

    pub(crate) fn window(mut self, history: &[Vec<Cell>], cols: u16) -> &[Vec<Cell>] {
        let keep = history
            .iter()
            .rev()
            .take_while(|row| {
                self.take_row(
                    cols,
                    row.iter()
                        .filter_map(|cell| cell.extra_text.as_ref().map(|text| text.len())),
                )
            })
            .count();
        &history[history.len() - keep..]
    }
}
