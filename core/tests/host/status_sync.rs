use super::*;
use pigeon_core::api;

fn complete(client: &mut Client, revision: u64) -> api::StatusSnapshot {
    let message = client
        .wait_for(|m| matches!(m, Message::StatusSnapshot { revision: r, .. } if *r >= revision));
    api::status_snapshot_of(&message).unwrap().1
}

fn emit(client: &mut Client, pane: u32, sequences: &str) {
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: format!("printf '{sequences}'\r").into_bytes(),
    });
}

#[test]
fn complete_status_ack_prompt_clear_reconnect_and_legacy_share_lifecycle() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let pane = attach_first(&mut worker);
    let mut observer = Client::connect(&host.socket);
    let offered = hello_with(
        &mut observer,
        &["status_all", "status_snapshot", "future_status"],
    );
    assert!(offered.iter().any(|f| f == "status_snapshot"));
    assert!(!offered.iter().any(|f| f == "future_status"));
    assert!(complete(&mut observer, 0).records.is_empty());

    emit(
        &mut worker,
        pane,
        "\\033]7501;id=a:state=done\\007\\033]7501;id=b:state=error\\007",
    );
    let before = complete(&mut observer, 2);
    assert_eq!(before.records.len(), 2);
    worker.wait_for(|m| matches!(m, Message::Status { seq: 2, .. }));
    observer.send(&Message::AckStatus {
        pane_id: pane,
        seq: 2,
    });
    // No further PTY output or query is needed to reveal the older record.
    let after = complete(&mut observer, before.revision + 1);
    assert_eq!(after.records.len(), 1);
    assert_eq!(after.records[0].seq, 1);
    worker.wait_for(|m| matches!(m, Message::Status { seq: 1, .. }));

    emit(&mut worker, pane, "\\033]7501;id=x:state=blocked:kind=permission\\007\\033]7501;id=y:state=blocked:kind=auth\\007");
    let blocked = complete(&mut observer, after.revision + 2);
    assert_eq!(blocked.records.len(), 3);
    emit(&mut worker, pane, "\\033]133;A\\007");
    let prompted = complete(&mut observer, blocked.revision + 1);
    assert_eq!(prompted.records.len(), 1);
    assert_eq!(prompted.records[0].id, "a");

    drop(observer);
    let mut observer = Client::connect(&host.socket);
    hello_with(&mut observer, &["status_all", "status_snapshot"]);
    let replay = complete(&mut observer, 0);
    assert_eq!(replay.revision, prompted.revision);
    assert_eq!(replay.records[0].seq, 1);
    emit(&mut worker, pane, "\\033]7501;state=clear:id=a\\007");
    let cleared = complete(&mut observer, replay.revision + 1);
    assert!(cleared.records.is_empty());
    worker.wait_for(|m| matches!(m, Message::Status { seq: 0, state, .. } if state == "idle"));

    observer.send(&Message::Ping { ts: 17 });
    let next = observer.wait_for(|_| true);
    assert!(
        matches!(next, Message::Pong { ts: 17 }),
        "new path must not also send legacy status: {next:?}"
    );
    worker.send(&Message::Ping { ts: 18 });
    let next =
        worker.wait_for(|m| matches!(m, Message::Pong { .. } | Message::StatusSnapshot { .. }));
    assert!(
        matches!(next, Message::Pong { ts: 18 }),
        "legacy client got a new message"
    );
    worker.send(&Message::ResizePane {
        pane_id: pane,
        cols: 90,
        rows: 30,
    });
    expect_size_state(&mut worker, pane, (90, 30, "latest", 1));
}

#[test]
fn complete_status_attached_scope_and_screen_resume_always_get_full_status() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello_with(&mut worker, &["delta", "status_snapshot"]);
    let pane = attach_first_with_policy(&mut worker, None);
    assert!(complete(&mut worker, 0).records.is_empty());
    emit(&mut worker, pane, "\\033]7501;id=a:state=done\\007");
    let pending = complete(&mut worker, 1);
    worker.send(&Message::Attach {
        id: 2,
        pane_id: pane,
        size_policy: None,
        history: Some(false),
        since: Some(0),
    });
    assert_eq!(complete(&mut worker, 1).revision, pending.revision);

    let mut unattached = Client::connect(&host.socket);
    hello_with(&mut unattached, &["status_snapshot"]);
    unattached.send(&Message::Ping { ts: 9 });
    let response = unattached.wait_for(|m| !matches!(m, Message::Sessions { .. }));
    assert!(matches!(response, Message::Pong { ts: 9 }));
    unattached.send(&Message::Attach {
        id: 3,
        pane_id: pane,
        size_policy: None,
        history: Some(false),
        since: None,
    });
    assert_eq!(complete(&mut unattached, 1).records[0].id, "a");
}

#[test]
fn process_exit_publishes_removal_to_unattached_status_observers() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut worker = Client::connect(&host.socket);
    hello(&mut worker);
    let pane = attach_first(&mut worker);
    let mut observer = Client::connect(&host.socket);
    hello_with(&mut observer, &["status_snapshot", "status_all"]);
    complete(&mut observer, 0);
    emit(
        &mut worker,
        pane,
        "\\033]7501;id=done:state=done\\007\\033]7501;id=live:state=blocked\\007",
    );
    let before = complete(&mut observer, 2);
    worker.send(&Message::Keys {
        pane_id: pane,
        bytes: b"exit\r".to_vec(),
    });
    let after = complete(&mut observer, before.revision + 1);
    assert_eq!(after.records.len(), 1);
    assert_eq!(after.records[0].id, "done");
}
