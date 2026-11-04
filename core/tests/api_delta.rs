//! Deltas through the same safe interface a Rust client uses without internals.

use pigeon_core::api::{delta_of, Message, PgError, Row, WireCell, WireCursor};

fn delta() -> Message {
    Message::Delta {
        pane_id: 7,
        seq: 9,
        rows: vec![Row {
            y: 2,
            cells: ['中', ' ', 'a']
                .map(|ch| WireCell {
                    codepoint: u32::from(ch),
                    attrs: 0,
                    fg: 0x0100_0007,
                    bg: 0x0100_0000,
                    link: None,
                })
                .to_vec(),
        }],
        cursor: Some(WireCursor {
            x: 2,
            y: 2,
            visible: true,
            shape: 1,
        }),
        scroll: None,
        links: None,
        alt: Some(true),
        modes: Some(3),
    }
}

#[test]
fn a_delta_preserves_its_rows_widths_cursor_and_modes() {
    let (pane, picture) = delta_of(&delta()).expect("delta");
    assert_eq!(pane, 7);
    assert_eq!(picture.seq, 9);
    assert_eq!(picture.alt, Some(true));
    assert_eq!(picture.modes, Some(3));
    let (y, cells) = &picture.rows[0];
    assert_eq!(*y, 2);
    assert_eq!(
        cells.iter().map(|cell| cell.width).collect::<Vec<_>>(),
        [2, 0, 1]
    );
    assert_eq!(picture.cursor.expect("cursor").shape, 1);
}

#[test]
fn a_non_delta_is_refused() {
    assert_eq!(
        delta_of(&Message::Pong { ts: 1 }).err(),
        Some(PgError::InvalidArg)
    );
}

#[test]
fn inconsistent_delta_row_widths_are_refused() {
    let mut message = delta();
    if let Message::Delta { rows, .. } = &mut message {
        rows.push(Row {
            y: 3,
            cells: vec![rows[0].cells[0]],
        });
    }
    assert_eq!(delta_of(&message).err(), Some(PgError::InvalidArg));
}

#[test]
fn a_cursor_only_delta_does_not_require_rows() {
    let mut message = delta();
    if let Message::Delta { rows, modes, .. } = &mut message {
        rows.clear();
        *modes = None;
    }
    let (_, picture) = delta_of(&message).expect("cursor-only delta");
    assert!(picture.rows.is_empty());
    assert!(picture.cursor.is_some());
    assert_eq!(picture.modes, None);
}

#[test]
fn duplicate_delta_rows_are_refused_by_both_interfaces() {
    use pigeon_core::ffi::{
        pg_last_error_code, pg_msg_decode_frame, pg_msg_delta, pg_msg_free, PgErrorCode,
    };

    let mut message = delta();
    if let Message::Delta { rows, .. } = &mut message {
        rows.push(rows[0].clone());
    }
    assert_eq!(delta_of(&message).err(), Some(PgError::InvalidArg));
    let frame = pigeon_core::api::encode_frame(&message).expect("frame");
    let mut consumed = 0;
    let handle = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
    assert!(!handle.is_null());
    assert!(pg_msg_delta(handle).is_null());
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    pg_msg_free(handle);
}
