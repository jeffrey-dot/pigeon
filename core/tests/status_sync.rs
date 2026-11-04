#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pigeon_core::api::{self, Config, Core, Message, PgError};

fn report(core: &mut Core, pane: u32, fields: &str) {
    core.feed(pane, format!("\x1b]7501;{fields}\x07").as_bytes())
        .unwrap();
}

#[test]
fn snapshot_reads_are_owned_non_consuming_and_ack_exposes_older_records() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(80, 24).unwrap();
    assert_eq!(core.status_snapshot(pane).unwrap().revision, 0);
    report(&mut core, pane, "id=a:state=done:msg=b2xk:app=pi");
    report(&mut core, pane, "id=b:state=error");
    let before = core.status_snapshot(pane).unwrap();
    assert_eq!(before.records.len(), 2);
    assert_eq!(
        core.status_snapshot(pane).unwrap().revision,
        before.revision
    );
    assert!(core.ack_status(pane, 2).unwrap());
    let after = core.status_snapshot(pane).unwrap();
    assert!(after.revision > before.revision);
    assert_eq!(after.records.len(), 1);
    assert_eq!(after.records[0].seq, 1);
    assert_eq!(after.records[0].msg, "old");
    assert_eq!(before.records.len(), 2, "owned snapshot survives mutation");
    assert!(
        !core.ack_status(pane, 2).unwrap(),
        "duplicate ack is a no-op"
    );
    assert_eq!(core.status_snapshot(pane).unwrap().revision, after.revision);
    assert_eq!(core.status_snapshot(999), Err(PgError::NoSuchPane));
}

#[test]
fn prompt_and_subtree_clear_replace_the_complete_set() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(80, 24).unwrap();
    for fields in [
        "id=a:state=done",
        "id=a.child:state=error",
        "id=ab:state=done",
        "id=live:state=blocked:kind=question",
        "id=other:state=blocked:kind=auth",
    ] {
        report(&mut core, pane, fields);
    }
    let before = core.status_snapshot(pane).unwrap();
    assert_eq!(before.records.len(), 5);
    assert!(!core.ack_status(pane, 4).unwrap());
    core.feed(pane, b"\x1b]133;A\x07").unwrap();
    let prompted = core.status_snapshot(pane).unwrap();
    assert_eq!(prompted.records.len(), 3);
    assert!(prompted.revision > before.revision);
    report(&mut core, pane, "state=clear:id=a");
    let cleared = core.status_snapshot(pane).unwrap();
    assert_eq!(cleared.records.len(), 1);
    assert_eq!(cleared.records[0].id, "ab");
    report(&mut core, pane, "state=clear");
    assert!(core.status_snapshot(pane).unwrap().records.is_empty());
}

#[test]
fn invalid_reports_and_capacity_do_not_silently_delete_unread() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(80, 24).unwrap();
    for index in 0..256 {
        report(&mut core, pane, &format!("id=r{index}:state=done"));
    }
    let before = core.status_snapshot(pane).unwrap();
    for bad in [
        "id=bad:state=error:msg=***".to_owned(),
        "id=bad:state=error:msg=AQ==".into(),
        format!("id=bad:state=done:app={}", "x".repeat(4096)),
        "id=overflow:state=error".into(),
    ] {
        report(&mut core, pane, &bad);
    }
    assert_eq!(
        core.status_snapshot(pane).unwrap().revision,
        before.revision
    );
    assert_eq!(core.status_snapshot(pane).unwrap().records.len(), 256);
    assert!(core.ack_status(pane, 1).unwrap());
    report(&mut core, pane, "id=new:state=error");
    assert_eq!(core.status_snapshot(pane).unwrap().records.len(), 256);
}

fn wire() -> Message {
    Message::StatusSnapshot {
        pane_id: 1,
        revision: 99,
        records: vec![api::WireStatusRecord {
            id: "job.child".into(),
            seq: 3,
            state: "blocked".into(),
            kind: "permission".into(),
            msg: "Allow?".into(),
            app: "pi".into(),
            age_ms: 500,
        }],
    }
}

#[test]
fn wire_snapshot_roundtrip_and_typed_read_share_validation() {
    let message = wire();
    assert_eq!(
        api::decode(&api::encode(&message).unwrap()).unwrap(),
        message
    );
    let (pane, snapshot) = api::status_snapshot_of(&message).unwrap();
    assert_eq!(pane, 1);
    assert_eq!(snapshot.revision, 99);
    assert_eq!(snapshot.records[0].seq, 3);
    assert_eq!(snapshot.records[0].age_ms, 500);
    assert_eq!(snapshot.records[0].kind, api::StatusKind::Permission);
    assert_eq!(
        api::status_snapshot_of(&Message::Ping { ts: 0 }),
        Err(PgError::InvalidArg)
    );
}

#[test]
fn wire_rejects_bad_records_duplicates_and_oversized_sets() {
    let mut cases = Vec::new();
    for field in [
        "state",
        "kind",
        "id",
        "msg",
        "app",
        "seq",
        "count",
        "duplicate",
        "size",
        "revision",
        "pane",
    ] {
        let mut message = wire();
        let Message::StatusSnapshot {
            pane_id,
            revision,
            records,
        } = &mut message
        else {
            unreachable!()
        };
        match field {
            "state" => records[0].state = "clear".into(),
            "kind" => records[0].kind = "bogus".into(),
            "id" => records[0].id = "bad\n".into(),
            "msg" => records[0].msg = "bad\x1b".into(),
            "app" => records[0].app = "bad\0".into(),
            "seq" => records[0].seq = 0,
            "count" => records.resize(257, records[0].clone()),
            "duplicate" => records.push(records[0].clone()),
            "size" => records[0].msg = "x".repeat(4097),
            "revision" => *revision = 0,
            "pane" => *pane_id = 0,
            _ => unreachable!(),
        }
        cases.push(message);
    }
    for message in cases {
        let mut bytes = Vec::new();
        ciborium::into_writer(&message, &mut bytes).unwrap();
        assert_eq!(api::decode(&bytes), Err(PgError::InvalidArg));
        assert_eq!(api::status_snapshot_of(&message), Err(PgError::InvalidArg));
        assert_eq!(api::encode(&message), Err(PgError::InvalidArg));
    }
}

#[test]
fn pane_scope_stale_ack_and_repeated_state_age_are_independent() {
    let mut core = Core::new(Config::default());
    let a = core.new_pane(80, 24).unwrap();
    let b = core.split(a, api::Axis::Vertical, 0.5).unwrap();
    report(&mut core, a, "id=job:state=done");
    report(&mut core, b, "id=job:state=blocked");
    assert!(!core.ack_status(b, 1).unwrap());
    assert!(core.ack_status(a, 1).unwrap());
    assert_eq!(core.status_snapshot(b).unwrap().records.len(), 1);
    report(&mut core, a, "id=job:state=done");
    assert!(
        !core.ack_status(a, 1).unwrap(),
        "ack of the replaced identity is stale"
    );
    assert_eq!(core.status_snapshot(a).unwrap().records[0].seq, 2);
    let before = core.status_snapshot(b).unwrap();
    report(&mut core, b, "id=job:state=blocked:msg=dXBkYXRl");
    let after = core.status_snapshot(b).unwrap();
    assert!(after.records[0].age_ms >= before.records[0].age_ms);
    assert!(after.revision > before.revision);
}

#[test]
fn wire_missing_null_wrong_type_and_duplicate_seq_are_rejected() {
    use ciborium::value::Value;
    let valid: Value = ciborium::from_reader(api::encode(&wire()).unwrap().as_slice()).unwrap();
    for field in ["id", "seq", "state", "kind", "msg", "app", "age_ms"] {
        for replacement in [None, Some(Value::Null), Some(Value::Bool(true))] {
            let Value::Map(mut root) = valid.clone() else {
                unreachable!()
            };
            let (_, Value::Array(records)) = root
                .iter_mut()
                .find(|(key, _)| key.as_text() == Some("records"))
                .unwrap()
            else {
                unreachable!()
            };
            let Value::Map(record) = &mut records[0] else {
                unreachable!()
            };
            record.retain(|(key, _)| key.as_text() != Some(field));
            if let Some(value) = replacement {
                record.push((Value::Text(field.into()), value));
            }
            let mut bytes = Vec::new();
            ciborium::into_writer(&Value::Map(root), &mut bytes).unwrap();
            assert_eq!(
                api::decode(&bytes),
                Err(PgError::InvalidArg),
                "field {field}"
            );
        }
    }
    let mut message = wire();
    let Message::StatusSnapshot { records, .. } = &mut message else {
        unreachable!()
    };
    let mut duplicate = records[0].clone();
    duplicate.id = "another".into();
    records.push(duplicate);
    assert_eq!(api::encode(&message), Err(PgError::InvalidArg));
}
