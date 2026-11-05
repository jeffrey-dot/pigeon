//! OSC 7501 sidebar derivation; collection semantics come from the safe core API.

use pigeon_core::api::{
    status_snapshot_of, Message, StatusSnapshot, StatusSnapshotRecord, StatusState as State,
};
use std::collections::{HashMap, HashSet};

pub mod grouping;
pub mod records;
pub mod rows;
#[cfg(test)]
mod tests;
pub mod view;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordRow {
    pub generation: u64,
    host: String,
    instance: Option<String>,
    pub pane: u32,
    pub record: StatusSnapshotRecord,
}

#[derive(Default)]
pub struct Summary {
    pub state: Option<State>,
    pub unread: usize,
    pub needs_you: bool,
    pub age_ms: Option<u64>,
}

#[derive(Default)]
pub struct SidebarModel {
    generation: u64,
    host: String,
    instance: Option<String>,
    complete: bool,
    reports: HashMap<u32, StatusSnapshot>,
    // Only identities still present in the current sets survive pruning.
    seen: HashSet<(u32, u32, String)>,
}

impl SidebarModel {
    pub fn begin_connection(&mut self, host: &str, instance: Option<&str>, complete: bool) {
        self.generation = self.generation.wrapping_add(1);
        self.host = host.into();
        self.instance = instance.map(str::to_owned);
        self.complete = complete;
        self.reports.clear();
        self.seen.clear();
    }

    pub fn apply(&mut self, message: &Message) {
        let next = if self.complete {
            status_snapshot_of(message).ok()
        } else {
            legacy(message)
        };
        let Some((pane, mut snapshot)) = next else {
            return;
        };
        if self.complete
            && self
                .reports
                .get(&pane)
                .is_some_and(|old| old.revision >= snapshot.revision)
        {
            return;
        }
        if !self.complete {
            // A same-id report supersedes its identity; other terminals need explicit viewing.
            if let Some(old) = self.reports.get(&pane) {
                for record in &old.records {
                    if record.state.is_terminal()
                        && !snapshot.records.iter().any(|r| r.id == record.id)
                        && !self.seen.contains(&(pane, record.seq, record.id.clone()))
                    {
                        snapshot.records.push(record.clone());
                    }
                }
            }
        }
        self.seen.retain(|(p, seq, id)| {
            *p != pane
                || snapshot
                    .records
                    .iter()
                    .any(|r| r.seq == *seq && r.id == *id)
        });
        self.reports.insert(pane, snapshot);
    }

    pub fn remove(&mut self, panes: &HashSet<u32>) {
        self.reports.retain(|pane, _| !panes.contains(pane));
        self.seen.retain(|(pane, _, _)| !panes.contains(pane));
    }

    pub fn records(&self, panes: &[u32]) -> Vec<RecordRow> {
        let mut rows: Vec<_> = panes
            .iter()
            .flat_map(|pane| {
                self.reports
                    .get(pane)
                    .into_iter()
                    .flat_map(move |snapshot| {
                        snapshot.records.iter().map(move |record| RecordRow {
                            generation: self.generation,
                            host: self.host.clone(),
                            instance: self.instance.clone(),
                            pane: *pane,
                            record: record.clone(),
                        })
                    })
            })
            .collect();
        rows.sort_by(|a, b| {
            (priority(a.record.state), a.pane, &a.record.id).cmp(&(
                priority(b.record.state),
                b.pane,
                &b.record.id,
            ))
        });
        rows
    }

    pub fn summary(&self, panes: &[u32]) -> Summary {
        let rows = self.records(panes);
        let unread = rows
            .iter()
            .filter(|row| {
                row.record.state.is_terminal()
                    && !self
                        .seen
                        .contains(&(row.pane, row.record.seq, row.record.id.clone()))
            })
            .count();
        Summary {
            state: rows.first().map(|r| r.record.state),
            age_ms: rows.first().map(|r| r.record.age_ms),
            unread,
            needs_you: unread > 0 || rows.iter().any(|r| r.record.state == State::Blocked),
        }
    }

    pub fn current(&self, row: &RecordRow) -> bool {
        row.generation == self.generation
            && row.host == self.host
            && row.instance == self.instance
            && self.reports.get(&row.pane).is_some_and(|s| {
                s.records
                    .iter()
                    .any(|r| r.id == row.record.id && r.seq == row.record.seq)
            })
    }

    pub fn can_ack(&self, row: &RecordRow, visible: bool) -> bool {
        visible
            && self.current(row)
            && row.record.state.is_terminal()
            && !self
                .seen
                .contains(&(row.pane, row.record.seq, row.record.id.clone()))
    }

    pub fn ack(&mut self, row: &RecordRow, visible: bool) -> Option<Message> {
        if !self.can_ack(row, visible) {
            return None;
        }
        self.seen
            .insert((row.pane, row.record.seq, row.record.id.clone()));
        Some(Message::AckStatus {
            pane_id: row.pane,
            seq: row.record.seq,
        })
    }
}

pub fn priority(state: State) -> u8 {
    match state {
        State::Blocked => 0,
        State::Error => 1,
        State::Done => 2,
        State::Working => 3,
        State::Idle => 4,
    }
}

fn legacy(message: &Message) -> Option<(u32, StatusSnapshot)> {
    let Message::Status {
        pane_id,
        state,
        seq,
        kind,
        msg,
        app,
        id,
        age_ms,
    } = message
    else {
        return None;
    };
    // Reuse complete-set validation/conversion rather than adding a second parser.
    let records = if *seq == 0 {
        vec![]
    } else {
        vec![pigeon_core::api::WireStatusRecord {
            id: id.clone().unwrap_or_default(),
            seq: *seq,
            state: state.clone(),
            kind: kind.clone().unwrap_or_default(),
            msg: msg.clone().unwrap_or_default(),
            app: app.clone().unwrap_or_default(),
            age_ms: age_ms.unwrap_or(0),
        }]
    };
    status_snapshot_of(&Message::StatusSnapshot {
        pane_id: *pane_id,
        revision: u64::from(*seq),
        records,
    })
    .ok()
}
