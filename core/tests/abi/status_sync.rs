use pigeon_core::{api, ffi::*};
use std::{mem::size_of, ptr};

fn record() -> PgStatusSnapshotRecord {
    // SAFETY: all fields are integers, pointers, or enums whose zero discriminant is valid.
    let mut out: PgStatusSnapshotRecord = unsafe { std::mem::zeroed() };
    out.size = size_of::<PgStatusSnapshotRecord>() as u32;
    out
}

fn info(snapshot: *const PgStatusSnapshot) -> PgStatusSnapshotInfo {
    let mut out = PgStatusSnapshotInfo {
        size: size_of::<PgStatusSnapshotInfo>() as u32,
        version: 0,
        pane_id: 0,
        revision: 0,
        count: 0,
    };
    assert!(pg_status_snapshot_info(snapshot, &mut out));
    out
}

fn text(raw: PgStr) -> String {
    // SAFETY: the caller keeps the snapshot alive while copying the string.
    String::from_utf8(unsafe { std::slice::from_raw_parts(raw.ptr, raw.len) }.to_vec()).unwrap()
}

#[test]
fn status_handles_own_complete_records_and_survive_core_mutation_and_free() {
    let core = pg_core_new(ptr::null());
    let pane = pg_pane_new(core, ptr::null());
    let bytes = b"\x1b]7501;id=a:state=done:msg=b2xk:app=pi\x07\x1b]7501;id=b:state=error\x07";
    assert!(pg_pane_feed(core, pane, bytes.as_ptr(), bytes.len()));
    let snapshot = pg_status_snapshot_new(core, pane);
    assert!(!snapshot.is_null());
    assert_eq!(info(snapshot).count, 2);
    let mut first = record();
    assert!(pg_status_snapshot_at(snapshot, 0, &mut first));
    assert_eq!(text(first.id), "a");
    assert_eq!(text(first.msg), "old");
    assert_eq!(text(first.app), "pi");
    assert_eq!(first.seq, 1);
    assert!(pg_status_ack(core, pane, 2));
    let after = pg_status_snapshot_new(core, pane);
    assert_eq!(info(after).count, 1);
    assert!(info(after).revision > info(snapshot).revision);
    pg_core_free(core);
    assert_eq!(info(snapshot).count, 2);
    assert!(pg_status_snapshot_at(snapshot, 0, &mut first));
    assert_eq!(text(first.msg), "old");
    pg_status_snapshot_free(after);
    pg_status_snapshot_free(snapshot);
    pg_status_snapshot_free(ptr::null_mut());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_OK);
}

#[test]
fn status_wire_accessor_copies_and_checks_pointer_size_and_index() {
    let frame = api::encode_frame(&api::Message::StatusSnapshot {
        pane_id: 5,
        revision: 20,
        records: vec![api::WireStatusRecord {
            id: "".into(),
            seq: 1,
            state: "blocked".into(),
            kind: "auth".into(),
            msg: "Login".into(),
            app: "agent".into(),
            age_ms: 900,
        }],
    })
    .unwrap();
    let mut used = 0;
    let msg = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut used);
    assert!(!msg.is_null());
    assert_eq!(pg_msg_type(msg), PgMsgType::PG_MSG_STATUS_SNAPSHOT);
    let snapshot = pg_msg_status_snapshot(msg);
    pg_msg_free(msg);
    assert_eq!(info(snapshot).pane_id, 5);
    assert_eq!(info(snapshot).revision, 20);
    let mut out = record();
    assert!(pg_status_snapshot_at(snapshot, 0, &mut out));
    assert_eq!(out.age_ms, 900);
    assert_eq!(out.kind, PgStatusKind::PG_KIND_AUTH);
    assert_eq!(text(out.id), "");
    assert!(!pg_status_snapshot_at(snapshot, 1, &mut out));
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    out.size = 4;
    assert!(!pg_status_snapshot_at(snapshot, 0, &mut out));
    assert!(!pg_status_snapshot_at(snapshot, 0, ptr::null_mut()));
    assert!(!pg_status_snapshot_at(ptr::null(), 0, &mut out));
    assert!(pg_msg_status_snapshot(ptr::null()).is_null());
    let ping = pg_msg_new(PgMsgType::PG_MSG_PING);
    assert!(pg_msg_status_snapshot(ping).is_null());
    pg_msg_free(ping);
    pg_status_snapshot_free(snapshot);
    let core = pg_core_new(ptr::null());
    assert!(pg_status_snapshot_new(core, 42).is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_NO_SUCH_PANE);
    pg_core_free(core);
}

#[test]
fn empty_snapshot_is_valid_and_larger_output_zeroes_tail() {
    let core = pg_core_new(ptr::null());
    let pane = pg_pane_new(core, ptr::null());
    let snapshot = pg_status_snapshot_new(core, pane);
    assert_eq!(info(snapshot).count, 0);
    #[repr(C)]
    struct Future {
        info: PgStatusSnapshotInfo,
        tail: [u8; 16],
    }
    let mut future = Future {
        info: info(snapshot),
        tail: [0xff; 16],
    };
    future.info.size = size_of::<Future>() as u32;
    assert!(pg_status_snapshot_info(snapshot, &mut future.info));
    assert_eq!(future.tail, [0; 16]);
    pg_status_snapshot_free(snapshot);
    pg_core_free(core);
}
