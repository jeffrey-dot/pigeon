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
    model.remove(&HashSet::from([1]));
    assert!(model.records(&[1]).is_empty());
    assert_eq!(model.ack(&old, true), None);
}

#[test]
fn legacy_same_id_replacement_invalidates_selection_without_evicting_other_unread() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", None, false);
    let status = |id: String, seq, state: &str| Message::Status {
        pane_id: 1,
        state: state.into(),
        seq,
        kind: None,
        msg: None,
        app: None,
        id: Some(id),
        age_ms: None,
    };
    for seq in 1..=256 {
        model.apply(&status(format!("r{seq}"), seq, "done"));
    }
    let old = model
        .records(&[1])
        .into_iter()
        .find(|r| r.record.id == "r256")
        .unwrap();
    model.apply(&status("r256".into(), 257, "working"));
    assert!(!model.current(&old));
    assert_eq!(model.ack(&old, true), None);
    assert_eq!(model.summary(&[1]).unread, 255);
    assert!(model.records(&[1]).iter().any(|r| r.record.id == "r1"));
}

#[test]
fn removing_confirmed_members_preserves_new_unknown_pane_status() {
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(99, 1, vec![record("new", 1, "done")]));
    model.apply(&report(1, 1, vec![record("old", 2, "done")]));
    model.remove(&HashSet::from([1]));
    assert_eq!(model.summary(&[99]).unread, 1);
    assert_eq!(model.summary(&[1]).unread, 0);
}

#[test]
fn row_derivation_sorts_needs_you_and_keeps_distinct_project_paths() {
    let session = |id, cwd: &str| pigeon_core::api::SessionInfo {
        session_id: id,
        title: format!("Session {id}"),
        cwd: cwd.into(),
        panes: vec![id],
        active_pane: Some(id),
        active_title: None,
        active_program: None,
    };
    let sessions = vec![
        session(1, "/a/proj"),
        session(2, "/a/proj/sub"),
        session(3, "/b/proj"),
    ];
    let projects = HashMap::from([
        ("/a/proj".into(), grouping::project("/a/proj", None)),
        (
            "/a/proj/sub".into(),
            grouping::project("/a/proj/sub", Some("/a/proj")),
        ),
    ]);
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(2, 1, vec![record("a", 1, "blocked")]));
    let rows = model.rows(&sessions, &projects, &HashMap::new());
    assert_eq!(
        rows.iter().filter_map(|r| r.session).collect::<Vec<_>>(),
        [2, 1, 3]
    );
    assert_eq!(rows.iter().filter(|r| r.session.is_none()).count(), 2);
    assert!(rows[0].text.contains("1 needs you"));
    assert_eq!(model.next_needed(&sessions, Some(3)), Some(2));
    assert_eq!(rows::adjacent(&sessions, Some(1), true), Some(3));
    assert_eq!(model.next_needed(&[], None), None);
}

#[test]
fn harp_row_keeps_title_age_status_and_hidden_details_separate() {
    let session = pigeon_core::api::SessionInfo {
        session_id: 4,
        title: "session 4".into(),
        cwd: "/code/launch".into(),
        panes: vec![4],
        active_pane: Some(4),
        active_title: Some("landing-hero <literal>".into()),
        active_program: Some("agent".into()),
    };
    let mut model = SidebarModel::default();
    model.begin_connection("host", Some("one"), true);
    model.apply(&report(4, 1, vec![record("a", 1, "done")]));
    let rows = model.rows(&[session], &HashMap::new(), &HashMap::new());
    assert_eq!(rows[1].name, "landing-hero <literal>");
    assert_eq!(rows[1].age, "now");
    assert!(rows[1].unread);
    assert_eq!(rows[1].state, Some(State::Done));
    assert!(rows[1].tooltip.contains("/code/launch"));
    assert!(!rows[1].name.contains("#4"));
}
