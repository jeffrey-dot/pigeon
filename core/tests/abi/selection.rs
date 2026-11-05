use pigeon_core::ffi::*;
use std::{mem::size_of, ptr};
#[test]
fn selection_owns_text_after_snapshot_and_core_are_freed() {
    let core = pg_core_new(ptr::null());
    let pane = pg_pane_new(core, ptr::null());
    let bytes = "a中e\u{301}".as_bytes();
    assert!(pg_pane_feed(core, pane, bytes.as_ptr(), bytes.len()));
    let snapshot = pg_pane_snapshot(core, pane);
    let request = PgGridSelection {
        size: size_of::<PgGridSelection>() as u32,
        version: PG_CORE_ABI_VERSION,
        anchor_row: 0,
        anchor_col: 2,
        head_row: 0,
        head_col: 3,
        unit: 0,
        rectangle: false,
    };
    let selected = pg_select_grid(snapshot, &request);
    assert!(!selected.is_null());
    pg_snapshot_free(snapshot);
    pg_core_free(core);
    let text = pg_selection_text(selected);
    assert_eq!(
        unsafe { std::slice::from_raw_parts(text.ptr, text.len) },
        "中e\u{301}".as_bytes()
    );
    assert_eq!(pg_selection_spans(selected, ptr::null_mut(), 0), 1);
    let mut span = PgSelectionSpan {
        size: 0,
        row: 0,
        first: 0,
        end: 0,
    };
    assert_eq!(pg_selection_spans(selected, &mut span, 1), 1);
    assert_eq!((span.first, span.end), (1, 4));
    pg_selection_free(selected);
    assert!(pg_select_grid(ptr::null(), &request).is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
}

#[test]
fn invalid_selection_request_does_not_create_an_owned_handle() {
    let core = pg_core_new(ptr::null());
    let pane = pg_pane_new(core, ptr::null());
    let snapshot = pg_pane_snapshot(core, pane);
    let mut request = PgGridSelection {
        size: size_of::<PgGridSelection>() as u32,
        version: PG_CORE_ABI_VERSION,
        anchor_row: 0,
        anchor_col: 0,
        head_row: 0,
        head_col: 1,
        unit: 99,
        rectangle: false,
    };
    assert!(pg_select_grid(snapshot, &request).is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    request.unit = 0;
    request.size = 4;
    assert!(pg_select_grid(snapshot, &request).is_null());
    request.size = size_of::<PgGridSelection>() as u32;
    request.version = 99;
    assert!(pg_select_grid(snapshot, &request).is_null());
    pg_snapshot_free(snapshot);
    pg_core_free(core);
}
