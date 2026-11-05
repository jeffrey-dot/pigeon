//! OSC 7501 status aggregation for the sidebar. Collection semantics come from the safe core
//! API (`status_snapshot_of`); this mirrors the Linux client's `sidebar::SidebarModel`, trimmed
//! to what the sidebar draws: per-session state, unread, needs-you and relative age.
//!
//! Cross-restart `seen` persistence and explicit per-record acknowledgement (the Status Records
//! panel) are later stages; "unread" here means a terminal record not yet acked this session.

use pigeon_core::api::{status_snapshot_of, Message, StatusSnapshot, StatusState as State};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// What one session's row shows.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub state: Option<State>,
    pub unread: usize,
    pub age_ms: Option<u64>,
}

impl Summary {
    /// Same rule as the macOS `SidebarModel.needsYou`: blocked, errored, or finished-and-unread.
    pub fn needs_you(&self) -> bool {
        match self.state {
            Some(State::Blocked | State::Error) => true,
            Some(State::Done) => self.unread > 0,
            _ => false,
        }
    }
}

#[derive(Default)]
pub struct StatusModel {
    /// The latest complete set per pane and when it arrived; `age_ms` is relative to that.
    reports: HashMap<u32, (StatusSnapshot, Instant)>,
    /// Terminal records the user has acknowledged this session: `(pane, seq, id)`.
    seen: HashSet<(u32, u32, String)>,
}

impl StatusModel {
    /// Folds a `status_snapshot` message; ignores anything else or a stale revision.
    pub fn apply(&mut self, message: &Message) -> bool {
        self.apply_at(message, Instant::now())
    }

    fn apply_at(&mut self, message: &Message, now: Instant) -> bool {
        let Ok((pane, snapshot)) = status_snapshot_of(message) else {
            return false;
        };
        if self
            .reports
            .get(&pane)
            .is_some_and(|(old, _)| old.revision >= snapshot.revision)
        {
            return false;
        }
        self.seen.retain(|(p, seq, id)| {
            *p != pane
                || snapshot
                    .records
                    .iter()
                    .any(|r| r.seq == *seq && r.id == *id)
        });
        self.reports.insert(pane, (snapshot, now));
        true
    }

    /// The aggregate for a session's panes now.
    pub fn summary(&self, panes: &[u32]) -> Summary {
        self.summary_at(panes, Instant::now())
    }

    /// The most urgent record's state and its age (grown by the time since it arrived), plus the
    /// unread terminal count across the panes.
    fn summary_at(&self, panes: &[u32], now: Instant) -> Summary {
        let mut best: Option<(u8, u32, &str, State, u64)> = None;
        let mut unread = 0;
        for pane in panes {
            let Some((snapshot, received)) = self.reports.get(pane) else {
                continue;
            };
            let elapsed = now.saturating_duration_since(*received);
            for record in &snapshot.records {
                if record.state.is_terminal()
                    && !self.seen.contains(&(*pane, record.seq, record.id.clone()))
                {
                    unread += 1;
                }
                let age = record.age_ms.saturating_add(millis(elapsed));
                let key = (priority(record.state), *pane, record.id.as_str());
                if best.is_none_or(|b| key < (b.0, b.1, b.2)) {
                    best = Some((key.0, key.1, key.2, record.state, age));
                }
            }
        }
        Summary {
            state: best.map(|b| b.3),
            age_ms: best.map(|b| b.4),
            unread,
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Sort precedence: blocked is most urgent, idle least (matches the Linux client).
pub fn priority(state: State) -> u8 {
    match state {
        State::Blocked => 0,
        State::Error => 1,
        State::Done => 2,
        State::Working => 3,
        State::Idle => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::api::WireStatusRecord;

    fn snapshot(pane: u32, revision: u64, records: Vec<WireStatusRecord>) -> Message {
        Message::StatusSnapshot {
            pane_id: pane,
            revision,
            records,
        }
    }
    fn record(id: &str, seq: u32, state: &str, age_ms: u64) -> WireStatusRecord {
        WireStatusRecord {
            id: id.to_owned(),
            seq,
            state: state.to_owned(),
            kind: String::new(),
            msg: String::new(),
            app: String::new(),
            age_ms,
        }
    }

    #[test]
    fn blocked_outranks_working_and_needs_you() {
        let mut model = StatusModel::default();
        model.apply(&snapshot(
            1,
            1,
            vec![
                record("a", 1, "working", 60_000),
                record("b", 2, "blocked", 0),
            ],
        ));
        let summary = model.summary(&[1]);
        assert_eq!(summary.state, Some(State::Blocked));
        assert!(summary.needs_you());
    }

    #[test]
    fn unread_done_needs_you_until_the_record_goes() {
        let mut model = StatusModel::default();
        model.apply(&snapshot(1, 1, vec![record("done", 5, "done", 0)]));
        assert_eq!(model.summary(&[1]).unread, 1);
        assert!(model.summary(&[1]).needs_you());
        model.apply(&snapshot(1, 2, vec![]));
        assert_eq!(model.summary(&[1]), Summary::default());
    }

    #[test]
    fn working_and_idle_do_not_need_you() {
        let mut model = StatusModel::default();
        model.apply(&snapshot(1, 1, vec![record("w", 1, "working", 0)]));
        model.apply(&snapshot(2, 1, vec![record("i", 1, "idle", 0)]));
        assert!(!model.summary(&[1]).needs_you());
        assert!(!model.summary(&[2]).needs_you());
    }

    #[test]
    fn stale_revision_is_ignored_and_malformed_state_is_rejected() {
        let mut model = StatusModel::default();
        model.apply(&snapshot(1, 5, vec![record("x", 1, "working", 0)]));
        assert!(!model.apply(&snapshot(1, 3, vec![])));
        assert_eq!(model.summary(&[1]).state, Some(State::Working));
        // The core rejects an unknown state, so the set is not applied.
        assert!(!model.apply(&snapshot(1, 9, vec![record("x", 1, "sleeping", 0)])));
        assert_eq!(model.summary(&[1]).state, Some(State::Working));
    }

    #[test]
    fn age_grows_with_time_since_the_report_arrived() {
        let mut model = StatusModel::default();
        let start = Instant::now();
        model.apply_at(
            &snapshot(1, 1, vec![record("w", 1, "working", 30_000)]),
            start,
        );
        let later = start + Duration::from_secs(90);
        assert_eq!(model.summary_at(&[1], later).age_ms, Some(120_000));
    }
}
