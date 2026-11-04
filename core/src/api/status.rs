//! Complete status reads shared by the safe API and C ABI.

use super::{Core, Message, PaneId, PgResult, StatusSnapshot};

impl Core {
    /// Samples all unacknowledged records for this pane, independently of focus.
    /// Returns owned text and frozen ages; no consumption, acknowledgement or mutation.
    /// An empty set is a successful read. A missing pane returns `NoSuchPane`.
    pub fn status_snapshot(&self, pane_id: PaneId) -> PgResult<StatusSnapshot> {
        Ok(self.pane(pane_id)?.status().snapshot())
    }
}

/// Validates and copies a complete wire status set. Wrong message type, duplicate ids/seqs,
/// invalid state/kind/text, zero pane/seq or exceeded bounds return `InvalidArg`.
/// The result is independent of the message lifetime; omitted records mean deletion.
pub fn status_snapshot_of(message: &Message) -> PgResult<(PaneId, StatusSnapshot)> {
    crate::proto::status_snapshot_of(message)
}
