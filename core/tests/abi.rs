//! ABI skeleton: lifecycle, error reporting and the promise that a successful
//! call clears the thread-local error.

use std::ptr;

use pigeon_core::ffi::{
    pg_core_abi_version, pg_core_free, pg_core_new, pg_last_error, pg_last_error_code, PgConfig,
    PgErrorCode, PgStr,
};

fn error_text() -> String {
    // SAFETY: the pointer is valid until the next core call on this thread, and
    // we read it immediately.
    let raw = pg_last_error();
    let bytes = unsafe { std::slice::from_raw_parts(raw.ptr, raw.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn abi_version_is_one() {
    assert_eq!(pg_core_abi_version(), 1);
}

#[test]
fn core_new_free_roundtrip() {
    let core = pg_core_new(ptr::null());
    assert!(!core.is_null(), "null cfg must mean defaults");
    pg_core_free(core);
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_OK);
    // Freeing null is a no-op, not a crash.
    pg_core_free(ptr::null_mut());
}

#[test]
fn stale_error_is_cleared_by_the_next_successful_call() {
    // A valid core with no panes: pane 0 cannot exist.
    let core = pg_core_new(ptr::null());
    assert!(!core.is_null());

    let missing = pigeon_core::ffi::pg_layout_close(core, 42);
    assert!(!missing);
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_NO_SUCH_PANE);
    assert!(!error_text().is_empty());

    pg_core_free(core);
    assert_eq!(
        pg_last_error_code(),
        PgErrorCode::PG_OK,
        "a successful call must clear the previous error"
    );
    assert!(error_text().is_empty());
}

#[test]
fn config_size_below_the_abi_is_rejected() {
    let cfg = PgConfig {
        size: 4,
        version: 1,
        scrollback_lines: 0,
        osc7501_enabled: 1,
    };
    let core = pg_core_new(&cfg);
    assert!(core.is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
}

#[test]
fn config_version_from_the_future_is_rejected() {
    let cfg = PgConfig {
        size: std::mem::size_of::<PgConfig>() as u32,
        version: 99,
        scrollback_lines: 0,
        osc7501_enabled: 1,
    };
    let core = pg_core_new(&cfg);
    assert!(core.is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
}

#[test]
fn config_in_range_is_accepted() {
    let cfg = PgConfig {
        size: std::mem::size_of::<PgConfig>() as u32,
        version: 1,
        scrollback_lines: 500,
        osc7501_enabled: 1,
    };
    let core = pg_core_new(&cfg);
    assert!(!core.is_null());
    pg_core_free(core);
}

// ---------------------------------------------------------------- embedded

use pigeon_core::ffi::{
    pg_layout_close, pg_layout_focus, pg_layout_focused, pg_layout_rects, pg_pane_dirty_rows,
    pg_pane_feed, pg_pane_new, pg_pane_respond, pg_pane_snapshot, pg_snapshot_free, pg_status_at,
    pg_status_count, PgPaneOptions, PgRect, PgStatus, PgStatusState, PG_CORE_ABI_VERSION,
};

fn pane_options(cols: u16, rows: u16) -> PgPaneOptions {
    PgPaneOptions {
        size: std::mem::size_of::<PgPaneOptions>() as u32,
        version: PG_CORE_ABI_VERSION,
        cols,
        rows,
        title: PgStr {
            size: std::mem::size_of::<PgStr>() as u32,
            ptr: std::ptr::null(),
            len: 0,
        },
    }
}

#[test]
fn embedded_core_feeds_and_snapshots() {
    let core = pg_core_new(ptr::null());
    assert!(!core.is_null());
    let opts = pane_options(24, 4);
    let pane = pg_pane_new(core, &opts);
    assert!(pane > 0);

    assert!(pg_pane_feed(
        core,
        pane,
        "你好".as_bytes().as_ptr(),
        "你好".len()
    ));

    let snapshot = pg_pane_snapshot(core, pane);
    assert!(!snapshot.is_null());
    // SAFETY: owned here, freed below.
    let view = unsafe { &*snapshot };
    assert_eq!((view.cols, view.rows), (24, 4));
    assert_eq!(view.pane_id, pane);
    let row = unsafe { &*view.screen };
    let cells = row.cells;
    // Wide-char widths must survive the C boundary (the GUI draws from this).
    let first = unsafe { *cells };
    let second = unsafe { *cells.add(1) };
    assert_eq!(first.width, 2, "wide char owner");
    assert_eq!(second.width, 0, "continuation cell");
    assert_eq!(first.codepoint, '你' as u32);
    pg_snapshot_free(snapshot);

    // Dirty rows were produced by the feed.
    let mut rows = [0u16; 8];
    assert!(pg_pane_dirty_rows(core, pane, rows.as_mut_ptr(), rows.len()) > 0);

    // A layout with one pane reports the whole session box.
    let mut rect = PgRect {
        size: std::mem::size_of::<PgRect>() as u32,
        pane_id: 0,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    assert_eq!(pg_layout_rects(core, &mut rect, 1), 1);
    assert_eq!(rect.pane_id, pane);
    assert_eq!((rect.w, rect.h), (1.0, 1.0));

    assert_eq!(pg_layout_focused(core), pane);
    assert!(pg_layout_close(core, pane));
    assert!(
        !pg_layout_focus(core, pane),
        "closed pane cannot be focused"
    );
    assert!(!pg_pane_feed(core, pane, b"x".as_ptr(), 1));
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_NO_SUCH_PANE);

    pg_core_free(core);
}

#[test]
fn embedded_status_records_round_trip() {
    let core = pg_core_new(ptr::null());
    let opts = pane_options(40, 6);
    let pane = pg_pane_new(core, &opts);
    assert!(pane > 0);

    let report = b"\x1b]7501;state=blocked:kind=permission:msg=QWxsb3c=\x07";
    assert!(pg_pane_feed(core, pane, report.as_ptr(), report.len()));

    assert_eq!(pg_status_count(core), 1);
    let mut status = PgStatus {
        size: std::mem::size_of::<PgStatus>() as u32,
        version: 0,
        pane_id: 0,
        seq: 0,
        state: PgStatusState::PG_STATE_IDLE,
        kind: pigeon_core::ffi::PgStatusKind::PG_KIND_NONE,
        msg: PgStr {
            size: 0,
            ptr: ptr::null(),
            len: 0,
        },
    };
    assert!(pg_status_at(core, 0, &mut status));
    assert_eq!(status.pane_id, pane);
    assert_eq!(status.state, PgStatusState::PG_STATE_BLOCKED);
    let message = unsafe { std::slice::from_raw_parts(status.msg.ptr, status.msg.len) };
    assert_eq!(String::from_utf8_lossy(message), "Allow");
    pg_core_free(core);
}

#[test]
fn replies_are_delivered_through_respond() {
    let core = pg_core_new(ptr::null());
    let opts = pane_options(20, 4);
    let pane = pg_pane_new(core, &opts);
    assert!(pane > 0);
    // DA1: CSI c -> "\x1b[?6c"-style answer queued for the host.
    let _ = pg_pane_feed(core, pane, b"\x1b[c".as_ptr(), 3);
    let mut buffer = [0u8; 64];
    let written = pg_pane_respond(core, pane, buffer.as_mut_ptr(), buffer.len());
    assert!(written > 0, "a DA1 query must be answered");
    assert_eq!(buffer[0], 0x1b, "replies start with ESC");
    pg_core_free(core);
}

#[test]
fn layout_splits_closes_and_focuses() {
    use pigeon_core::ffi::{
        pg_layout_resize, pg_layout_split, pg_layout_split_of, pg_pane_free, PgAxis, PgRect,
    };

    let core = pg_core_new(ptr::null());
    let first = pg_pane_new(core, &pane_options(80, 24));
    assert!(first > 0);

    // Extra panes come from a split, not from pg_pane_new.
    assert_eq!(pg_pane_new(core, &pane_options(80, 24)), 0);
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);

    let second = pg_layout_split(core, first, PgAxis::PG_AXIS_VERTICAL, 0.5);
    assert!(second > first, "split returns the new pane id");
    assert_eq!(pg_layout_focused(core), second, "the new pane takes focus");
    let split = pg_layout_split_of(core, second);
    assert!(split > 0, "the new pane knows its divider");
    assert_eq!(pg_layout_split_of(core, first), split);

    let new_rect = |pane_id: u32| PgRect {
        size: std::mem::size_of::<PgRect>() as u32,
        pane_id,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    let mut rects = [new_rect(0), new_rect(0), new_rect(0), new_rect(0)];
    assert_eq!(pg_layout_rects(core, rects.as_mut_ptr(), rects.len()), 2);
    assert_eq!(rects[0].pane_id, first);
    assert_eq!(rects[1].pane_id, second);
    assert!((rects[0].w - 0.5).abs() < 1e-5 && (rects[1].x - 0.5).abs() < 1e-5);
    let area: f32 = rects[..2].iter().map(|r| r.w * r.h).sum();
    assert!((area - 1.0).abs() < 1e-5, "panes tile the box");

    // Dragging the divider is clamped by the minimum pane width.
    assert!(pg_layout_resize(core, split, 0.99));
    assert_eq!(pg_layout_rects(core, rects.as_mut_ptr(), rects.len()), 2);
    assert!(rects[0].w <= 0.9 + 1e-5, "clamped, got {}", rects[0].w);
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_OK);

    // Focus can move back, and only to live panes.
    assert!(pg_layout_focus(core, first));
    assert_eq!(pg_layout_focused(core), first);
    assert!(!pg_layout_focus(core, 999));
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_NO_SUCH_PANE);

    // Closing the second pane lets the first fill the box again.
    assert!(pg_layout_close(core, second));
    assert_eq!(pg_layout_rects(core, rects.as_mut_ptr(), rects.len()), 1);
    assert_eq!((rects[0].w, rects[0].h), (1.0, 1.0));
    assert_eq!(pg_layout_focused(core), first);
    assert_eq!(pg_layout_split_of(core, first), 0, "no divider left");

    pg_pane_free(core, first);
    pg_core_free(core);
}

#[test]
fn layout_message_exposes_rects_and_splits() {
    use pigeon_core::ffi::{
        pg_msg_decode_frame, pg_msg_layout_focus, pg_msg_layout_rects, pg_msg_layout_splits,
        pg_msg_type, PgAxis, PgMsgType, PgSplit,
    };
    use pigeon_core::proto::{Message, Node, SplitAxis, SplitNode};

    let message = Message::Layout {
        session_id: 1,
        tree: Node::Split {
            split: SplitNode {
                id: 4,
                axis: SplitAxis::Vertical,
                ratio: 0.25,
                a: Box::new(Node::Leaf { leaf: 1 }),
                b: Box::new(Node::Split {
                    split: SplitNode {
                        id: 5,
                        axis: SplitAxis::Horizontal,
                        ratio: 0.5,
                        a: Box::new(Node::Leaf { leaf: 2 }),
                        b: Box::new(Node::Leaf { leaf: 3 }),
                    },
                }),
            },
        },
        zoom: None,
        focus: Some(2),
    };
    let frame = pigeon_core::proto::encode_frame(&message).expect("frame");
    let mut consumed = 0usize;
    let handle = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
    assert!(!handle.is_null());
    assert_eq!(pg_msg_type(handle), PgMsgType::PG_MSG_LAYOUT);
    assert_eq!(pg_msg_layout_focus(handle), 2);

    let mut rects = [PgRect {
        size: std::mem::size_of::<PgRect>() as u32,
        pane_id: 0,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    }; 4];
    assert_eq!(
        pg_msg_layout_rects(handle, rects.as_mut_ptr(), rects.len()),
        3
    );
    assert_eq!(rects[0].pane_id, 1);
    assert!(
        (rects[0].w - 0.25).abs() < 1e-5,
        "geometry comes from the core"
    );
    assert_eq!(rects[1].pane_id, 2);
    assert!((rects[1].x - 0.25).abs() < 1e-5);
    assert!((rects[1].h - 0.5).abs() < 1e-5);

    let mut splits = [PgSplit {
        size: std::mem::size_of::<PgSplit>() as u32,
        id: 0,
        axis: PgAxis::PG_AXIS_VERTICAL,
        ratio: 0.0,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    }; 4];
    assert_eq!(
        pg_msg_layout_splits(handle, splits.as_mut_ptr(), splits.len()),
        2
    );
    assert_eq!(splits[0].id, 4);
    assert_eq!(splits[0].axis, PgAxis::PG_AXIS_VERTICAL);
    assert!((splits[0].ratio - 0.25).abs() < 1e-5);
    assert_eq!(splits[1].axis, PgAxis::PG_AXIS_HORIZONTAL);
    assert!((splits[1].x - 0.25).abs() < 1e-5, "divider knows its box");

    pigeon_core::ffi::pg_msg_free(handle);
}

#[test]
fn delta_rows_cross_the_abi_with_cursor_and_alt() {
    use pigeon_core::ffi::{pg_delta_free, pg_msg_decode_frame, pg_msg_delta, pg_msg_free};
    use pigeon_core::proto;
    use pigeon_core::session::PaneState;

    let mut pane = PaneState::new(7, 6, 3, 0);
    let _ = pane.snapshot_message(false);
    pane.feed("a你".as_bytes());
    let message = pane.delta_message().expect("the feed dirtied a row");
    let frame = proto::encode_frame(&message).expect("frame");
    let mut consumed = 0usize;
    let handle = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
    assert!(!handle.is_null());

    let delta = pg_msg_delta(handle);
    assert!(!delta.is_null(), "{}", error_text());
    // SAFETY: owned here, freed below.
    let view = unsafe { &*delta };
    assert_eq!(view.pane_id, 7);
    assert_eq!(view.seq, 1, "first delta after a snapshot at seq 0");
    assert!(view.row_count >= 1);
    let row = unsafe { &*view.rows };
    assert_eq!((row.y, row.cols), (0, 6));
    let cells = unsafe { std::slice::from_raw_parts(row.cells, 6) };
    assert_eq!(cells[0].codepoint, 'a' as u32);
    assert_eq!((cells[1].codepoint, cells[1].width), ('你' as u32, 2));
    assert_eq!(cells[2].width, 0, "continuation derived like a snapshot's");
    assert_eq!(view.has_cursor, 1);
    assert_eq!((view.cursor.x, view.cursor.y), (3, 0));
    assert_eq!(view.alt_screen, 0);
    pg_delta_free(delta);
    pg_msg_free(handle);
}

#[test]
fn a_non_delta_message_yields_no_delta() {
    use pigeon_core::ffi::{pg_delta_free, pg_msg_delta, pg_msg_free, pg_msg_new, PgMsgType};

    let msg = pg_msg_new(PgMsgType::PG_MSG_SNAPSHOT);
    assert!(pg_msg_delta(msg).is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    // An empty delta handle lacks `rows`: rejected by the schema, not read as zero rows.
    let empty = pg_msg_new(PgMsgType::PG_MSG_DELTA);
    assert!(pg_msg_delta(empty).is_null());
    assert!(pg_msg_delta(std::ptr::null()).is_null());
    pg_delta_free(std::ptr::null_mut());
    pg_msg_free(msg);
    pg_msg_free(empty);
}
