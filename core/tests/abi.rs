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
