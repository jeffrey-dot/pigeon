//! ABI skeleton: lifecycle, error reporting and the promise that a successful
//! call clears the thread-local error.

use std::ptr;

use pigeon_core::ffi::{
    pg_core_abi_version, pg_core_free, pg_core_new, pg_last_error, pg_last_error_code, PgConfig,
    PgErrorCode,
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
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_UNSUPPORTED);
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
