//! Immutable samples of the pending OSC 7501 record set. Lifecycle stays in StatusStore.

use super::{Kind, State, StatusStore};
use std::time::Instant;

/// Complete pending set: replace the previous set, including when `records` is empty.
/// Reads neither acknowledge records nor advance `revision`. Records are in id order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshot {
    /// Collection revision, independent of record seq and screen seq; scoped to a pane
    /// in one host instance. Gaps are legal because every sample is complete.
    pub revision: u64,
    pub records: Vec<StatusSnapshotRecord>,
}

/// Owned record sampled at snapshot creation. `age_ms` does not tick after sampling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshotRecord {
    pub id: String,
    pub seq: u32,
    pub state: State,
    pub kind: Kind,
    pub msg: String,
    pub app: String,
    pub age_ms: u64,
}

impl StatusStore {
    pub fn snapshot(&self) -> StatusSnapshot {
        let now = Instant::now();
        StatusSnapshot {
            revision: self.revision(),
            records: self
                .records()
                .filter(|record| !record.acked)
                .map(|record| StatusSnapshotRecord {
                    id: record.id.clone(),
                    seq: record.seq,
                    state: record.state,
                    kind: record.kind,
                    msg: record.msg.clone(),
                    app: record.app.clone(),
                    age_ms: u64::try_from(now.saturating_duration_since(record.since).as_millis())
                        .unwrap_or(u64::MAX),
                })
                .collect(),
        }
    }
}
