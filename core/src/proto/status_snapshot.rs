//! `status_snapshot` wire shape and validation (docs/protocol.md §3.4).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::Message;
use crate::error::{PgError, PgResult};
use crate::status::{Kind, State, StatusSnapshot, StatusSnapshotRecord, MAX_RECORDS};

/// Wire record; all fields are required, with empty strings denoting absent text/kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireStatusRecord {
    pub id: String,
    pub seq: u32,
    pub state: String,
    pub kind: String,
    pub msg: String,
    pub app: String,
    pub age_ms: u64,
}

/// Validates locally constructed messages as well as decoded wire input.
pub fn snapshot_of(message: &Message) -> PgResult<(u32, StatusSnapshot)> {
    let (pane_id, revision, records) = validated_records(message)?;
    let mut decoded = records
        .iter()
        .map(|record| {
            let (state, kind) = record_tags(record)?;
            Ok(StatusSnapshotRecord {
                id: record.id.clone(),
                seq: record.seq,
                state,
                kind,
                msg: record.msg.clone(),
                app: record.app.clone(),
                age_ms: record.age_ms,
            })
        })
        .collect::<PgResult<Vec<_>>>()?;
    // Producers use id order; receivers accept any order without assigning UI priority.
    decoded.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((pane_id, StatusSnapshot { revision, records: decoded }))
}

/// Validation borrows strings; encoding/decoding must not build and discard an owned sample.
fn validated_records(message: &Message) -> PgResult<(u32, u64, &[WireStatusRecord])> {
    let Message::StatusSnapshot {
        pane_id,
        revision,
        records,
    } = message
    else {
        return Err(PgError::InvalidArg);
    };
    if *pane_id == 0 || records.len() > MAX_RECORDS || (*revision == 0 && !records.is_empty()) {
        return Err(PgError::InvalidArg);
    }
    let mut ids = BTreeSet::new();
    let mut seqs = BTreeSet::new();
    for record in records {
        if record.seq == 0
            || !ids.insert(&record.id)
            || !seqs.insert(record.seq)
            || record.id.len() + record.msg.len() + record.app.len() > 4096
            || [&record.id, &record.msg, &record.app]
                .iter()
                .any(|text| text.chars().any(char::is_control))
        {
            return Err(PgError::InvalidArg);
        }
        record_tags(record)?;
    }
    Ok((*pane_id, *revision, records))
}

fn record_tags(record: &WireStatusRecord) -> PgResult<(State, Kind)> {
    let state = match record.state.as_str() {
        "idle" => State::Idle,
        "working" => State::Working,
        "blocked" => State::Blocked,
        "done" => State::Done,
        "error" => State::Error,
        _ => return Err(PgError::InvalidArg),
    };
    let kind = match record.kind.as_str() {
        "" => Kind::None,
        "permission" => Kind::Permission,
        "question" => Kind::Question,
        "auth" => Kind::Auth,
        _ => return Err(PgError::InvalidArg),
    };
    Ok((state, kind))
}

pub fn message(pane_id: u32, snapshot: StatusSnapshot) -> Message {
    Message::StatusSnapshot {
        pane_id,
        revision: snapshot.revision,
        records: snapshot
            .records
            .into_iter()
            .map(|record| WireStatusRecord {
                id: record.id,
                seq: record.seq,
                state: record.state.as_str().to_owned(),
                kind: record.kind.as_str().to_owned(),
                msg: record.msg,
                app: record.app,
                age_ms: record.age_ms,
            })
            .collect(),
    }
}

pub(super) fn validate(message: &Message) -> PgResult<()> {
    if matches!(message, Message::StatusSnapshot { .. }) {
        validated_records(message)?;
    }
    Ok(())
}
