use super::*;
use pigeon_core::api::WireStatusRecord;

fn record(id: &str, seq: u32, state: &str) -> WireStatusRecord {
    WireStatusRecord {
        id: id.into(),
        seq,
        state: state.into(),
        kind: String::new(),
        msg: format!("message {id}"),
        app: "test".into(),
        age_ms: 1000,
    }
}
fn report(pane: u32, revision: u64, records: Vec<WireStatusRecord>) -> Message {
    Message::StatusSnapshot {
        pane_id: pane,
        revision,
        records,
    }
}
#[test]
fn full_sets_aggregate_all_records_and_clear_without_ack() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(
        1,
        5,
        vec![
            record("a", 1, "blocked"),
            record("b", 2, "blocked"),
            record("c", 3, "done"),
        ],
    ));
    model.apply(&report(2, 8, vec![record("d", 4, "idle")]));
    assert_eq!(model.records(&[1, 2]).len(), 4);
    assert_eq!(model.summary(&[1, 2]).state, Some(State::Blocked));
    assert_eq!(model.summary(&[1, 2]).unread, 1);
    assert!(model.summary(&[1, 2]).needs_you);
    model.apply(&report(1, 6, vec![record("c", 3, "done")]));
    assert_eq!(model.summary(&[1, 2]).state, Some(State::Done));
    model.apply(&report(1, 7, vec![]));
    assert_eq!(model.summary(&[1]).state, None);
    assert_eq!(model.summary(&[1]).unread, 0);
}
#[test]
fn revision_is_connection_scoped_and_does_not_compare_record_seq() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(1, 10, vec![record("a", 80, "done")]));
    model.apply(&report(1, 11, vec![record("b", 2, "blocked")]));
    model.apply(&report(1, 10, vec![]));
    assert_eq!(model.summary(&[1]).state, Some(State::Blocked));
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(1, 11, vec![record("b", 2, "blocked")]));
    assert_eq!(model.records(&[1]).len(), 1);
    model.begin_connection("host", Some("two"), true);
    model.apply(&report(1, 1, vec![record("c", 1, "working")]));
    assert_eq!(model.summary(&[1]).state, Some(State::Working));
}
#[test]
fn explicit_ack_requires_exact_current_terminal_record_and_visibility() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(
        1,
        1,
        vec![record("a", 1, "done"), record("b", 2, "blocked")],
    ));
    let rows = model.records(&[1]);
    let done = rows
        .iter()
        .find(|row| row.record.id == "a")
        .unwrap()
        .clone();
    let blocked = rows
        .iter()
        .find(|row| row.record.id == "b")
        .unwrap()
        .clone();
    assert_eq!(model.ack(&done, false), None);
    assert_eq!(model.ack(&blocked, true), None);
    assert_eq!(
        model.ack(&done, true),
        Some(Message::AckStatus { pane_id: 1, seq: 1 })
    );
    assert_eq!(model.ack(&done, true), None);
    assert_eq!(model.summary(&[1]).unread, 0);
    model.apply(&report(1, 2, vec![record("a", 3, "error")]));
    assert_eq!(model.ack(&done, true), None);
    assert_eq!(model.summary(&[1]).unread, 1);
    let current = model.records(&[1]).remove(0);
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(1, 2, vec![record("a", 3, "error")]));
    assert_eq!(model.ack(&current, true), None);
}
#[test]
fn unnegotiated_or_malformed_complete_sets_are_ignored() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", None, false);
    model.apply(&report(1, 1, vec![record("a", 1, "done")]));
    assert!(model.records(&[1]).is_empty());
    model.begin_connection("host", None, true);
    model.apply(&report(1, 1, vec![record("a", 1, "bogus")]));
    assert!(model.records(&[1]).is_empty());
}

#[test]
fn legacy_live_report_does_not_claim_older_terminal_was_read() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", None, false);
    let status = |state: &str, seq| Message::Status {
        pane_id: 1,
        state: state.into(),
        seq,
        kind: None,
        msg: None,
        app: None,
        id: Some(format!("r{seq}")),
        age_ms: None,
    };
    model.apply(&status("done", 1));
    model.apply(&status("working", 2));
    assert_eq!(model.summary(&[1]).unread, 1);
    assert_eq!(model.records(&[1]).len(), 2);
}

#[test]
fn removed_pane_clears_records_and_old_inspection() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(1, 1, vec![record("a", 1, "error")]));
    let old = model.records(&[1]).remove(0);
    model.retain(&HashSet::new());
    assert!(model.records(&[1]).is_empty());
    assert_eq!(model.ack(&old, true), None);
}
