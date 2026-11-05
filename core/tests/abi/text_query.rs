use pigeon_core::ffi::*;
use std::{
    mem::{size_of, zeroed},
    ptr, slice,
};

fn request(pattern: &str) -> PgTextQuery {
    PgTextQuery {
        size: size_of::<PgTextQuery>() as u32,
        version: PG_CORE_ABI_VERSION,
        kind: 1,
        generation: 0,
        pattern: PgStr {
            size: size_of::<PgStr>() as u32,
            ptr: pattern.as_ptr(),
            len: pattern.len(),
        },
        case_sensitive: true,
        max_results: 128,
        start_set: false,
        range: unsafe { zeroed() },
    }
}

#[test]
fn text_query_handles_roundtrip_owned_text_and_stale_errors() {
    let core = pg_core_new(ptr::null());
    let mut options: PgPaneOptions = unsafe { zeroed() };
    options.size = size_of::<PgPaneOptions>() as u32;
    options.version = PG_CORE_ABI_VERSION;
    options.cols = 8;
    options.rows = 2;
    let pane = pg_pane_new(core, &options);
    let input = "abc你好e\u{301}Z\r\nnext\r\nend";
    assert!(pg_pane_feed(core, pane, input.as_ptr(), input.len()));
    let pattern = "abc你好e\u{301}Z\nnext";
    let mut query = request(pattern);
    let result = pg_text_query(core, pane, &query);
    assert!(!result.is_null());
    let mut info: PgTextResultInfo = unsafe { zeroed() };
    info.size = size_of::<PgTextResultInfo>() as u32;
    assert!(pg_text_result_info(result, &mut info));
    assert_eq!(info.count, 1);
    let mut range: PgTextRange = unsafe { zeroed() };
    range.size = size_of::<PgTextRange>() as u32;
    assert!(pg_text_result_hit(result, 0, &mut range));
    assert!(!pg_text_result_hit(result, 1, &mut range));
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    query.kind = 2;
    query.generation = info.generation;
    query.range = range;
    let message = pg_msg_text_query(42, pane, &query);
    assert!(!message.is_null());
    assert_eq!(pg_msg_type(message), PgMsgType::PG_MSG_TEXT_QUERY);
    let mut frame = vec![0; pg_msg_encoded_len(message)];
    assert_eq!(
        pg_msg_encode_frame(message, frame.as_mut_ptr(), frame.len()),
        frame.len()
    );
    let (decoded, _) = pigeon_core::api::decode_frame(&frame).unwrap().unwrap();
    assert!(
        matches!(decoded, pigeon_core::api::Message::TextQuery { id: 42, pane_id, .. } if pane_id == pane)
    );
    let text_result = pg_text_query(core, pane, &query);
    pg_msg_free(message);
    pg_text_result_free(result);
    let mut text: PgStr = unsafe { zeroed() };
    text.size = size_of::<PgStr>() as u32;
    assert!(pg_text_result_text(text_result, &mut text));
    assert_eq!(
        unsafe { slice::from_raw_parts(text.ptr, text.len) },
        pattern.as_bytes()
    );
    assert!(pg_pane_feed(core, pane, b"!".as_ptr(), 1));
    assert!(pg_text_query(core, pane, &query).is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_STALE_TEXT);
    // Result text belongs to its handle, including after core mutation and destruction.
    pg_core_free(core);
    assert_eq!(
        unsafe { slice::from_raw_parts(text.ptr, text.len) },
        pattern.as_bytes()
    );
    pg_text_result_free(text_result);
}

#[test]
fn text_query_rejects_bad_sizes_utf8_missing_fields_and_nulls() {
    let core = pg_core_new(ptr::null());
    let pane = pg_pane_new(core, ptr::null());
    let mut query = request("x");
    query.size = 4;
    assert!(pg_text_query(core, pane, &query).is_null());
    query.size = size_of::<PgTextQuery>() as u32;
    query.version = 99;
    assert!(pg_msg_text_query(1, pane, &query).is_null());
    query.version = PG_CORE_ABI_VERSION;
    query.pattern.ptr = b"\xff".as_ptr();
    assert!(pg_text_query(core, pane, &query).is_null());
    query.pattern.ptr = ptr::null();
    assert!(pg_text_query(core, pane, &query).is_null());
    assert!(pg_text_query(core, pane, ptr::null()).is_null());
    assert!(!pg_text_result_info(ptr::null(), ptr::null_mut()));
    assert!(pg_msg_text_result(ptr::null()).is_null());
    let empty = pg_msg_new(PgMsgType::PG_MSG_TEXT_QUERY);
    assert!(pg_msg_text_result(empty).is_null());
    pg_msg_free(empty);
    pg_text_result_free(ptr::null_mut());
    pg_core_free(core);
}

#[test]
fn text_result_from_wire_survives_message_free_and_rejects_wrong_type() {
    use pigeon_core::api::{self, Message, TextInfo, TextPoint, TextRange, TextResult};
    let result = Message::TextQueryResult {
        id: 43,
        pane_id: 7,
        result: TextResult::Search {
            info: TextInfo {
                generation: 9,
                first_row: -100,
                last_row: 23,
                cols: 80,
                alt: false,
            },
            hits: vec![TextRange {
                start: TextPoint {
                    row: -3,
                    col: 1,
                    byte: 0,
                },
                end: TextPoint {
                    row: -3,
                    col: 4,
                    byte: 1,
                },
            }],
            next: Some(TextPoint {
                row: -2,
                col: 0,
                byte: 0,
            }),
        },
    };
    let frame = api::encode_frame(&result).unwrap();
    let mut used = 0;
    let message = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut used);
    let handle = pg_msg_text_result(message);
    pg_msg_free(message);
    assert!(!handle.is_null());
    let mut info: PgTextResultInfo = unsafe { zeroed() };
    info.size = size_of::<PgTextResultInfo>() as u32;
    assert!(pg_text_result_info(handle, &mut info));
    assert_eq!(
        (info.id, info.pane_id, info.generation, info.next_row),
        (43, 7, 9, -2)
    );
    assert!(info.has_next);
    pg_text_result_free(handle);
}

#[test]
fn viewport_snapshot_is_owned_and_wrong_kind_is_rejected() {
    let core = pg_core_new(ptr::null());
    let pane = pg_pane_new(core, ptr::null());
    assert!(pg_pane_feed(core, pane, b"hello".as_ptr(), 5));
    let mut query = request("hello");
    let result = pg_text_query(core, pane, &query);
    assert!(pg_text_result_snapshot(result).is_null());
    let mut info: PgTextResultInfo = unsafe { zeroed() };
    info.size = size_of::<PgTextResultInfo>() as u32;
    assert!(pg_text_result_info(result, &mut info));
    query.range.size = size_of::<PgTextRange>() as u32;
    assert!(pg_text_result_hit(result, 0, &mut query.range));
    query.kind = 3;
    query.generation = info.generation;
    pg_text_result_free(result);
    let result = pg_text_query(core, pane, &query);
    let snapshot = pg_text_result_snapshot(result);
    assert!(!snapshot.is_null());
    let mut viewport: PgTextViewportInfo = unsafe { zeroed() };
    viewport.size = size_of::<PgTextViewportInfo>() as u32;
    assert!(pg_text_result_viewport(result, &mut viewport));
    assert_eq!(viewport.offset, 0);
    assert_eq!(viewport.history_len, 0);
    viewport.size = 4;
    assert!(!pg_text_result_viewport(result, &mut viewport));
    pg_text_result_free(result);
    pg_core_free(core);
    assert_eq!(unsafe { (*snapshot).history_count }, 0);
    assert_eq!(
        unsafe { (*(*snapshot).screen).cells.read().codepoint },
        'h' as u32
    );
    pg_snapshot_free(snapshot);
    assert!(pg_text_result_snapshot(ptr::null()).is_null());
}
