// Tests assert through public APIs, including the raw C ABI.
#![allow(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(feature = "internals")]

use pigeon_core::api::{
    decode_frame, delta_of, encode_frame, snapshot_of, CellText, Message, PgError,
};
use pigeon_core::ffi::*;
use pigeon_core::session::PaneState;

#[test]
fn marks_on_a_tab_use_the_exported_space_as_the_base() {
    let mut pane = PaneState::new(1, 8, 2, 0);
    // HT stores a tab in cell 0; position after it so the mark attaches there.
    pane.feed("\t\r\x1b[C\u{301}".as_bytes());
    let original = pane.snapshot();
    let message = pane.snapshot_message(false);
    let (_, picture) = snapshot_of(&message).expect("the core's own snapshot must be valid");
    assert_eq!(picture.screen, original.screen);
    assert_eq!(picture.screen[0][0].text(), " \u{301}");
    let delta = delta_of(&pane.delta_message().expect("delta")).expect("valid delta");
    assert_eq!(delta.rows[0].1[0].text(), " \u{301}");
}

#[test]
fn redundant_base_only_text_does_not_allocate_an_extra_string() {
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed(b"e");
    let mut message = pane.snapshot_message(false);
    if let Message::Snapshot { text, .. } = &mut message {
        text.push(CellText {
            row: 0,
            col: 0,
            text: "e".into(),
        });
    }
    let (_, snapshot) = snapshot_of(&message).expect("redundant but valid text");
    assert_eq!(snapshot.screen[0][0].text(), "e");
    assert!(snapshot.screen[0][0].extra_text.is_none());
}

fn with_text(mut message: Message, entries: Vec<CellText>) -> Message {
    match &mut message {
        Message::Snapshot { text, .. } | Message::Delta { text, .. } => *text = entries,
        _ => panic!("picture required"),
    }
    message
}

fn entry(row: i32, col: u16, text: &str) -> CellText {
    CellText {
        row,
        col,
        text: text.into(),
    }
}

fn reject_picture(message: &Message) {
    let snapshot = matches!(message, Message::Snapshot { .. });
    let result = if snapshot {
        snapshot_of(message).map(|_| ())
    } else {
        delta_of(message).map(|_| ())
    };
    assert_eq!(result, Err(PgError::InvalidArg));
    // CBOR/schema decoding and semantic picture validation are deliberately separate.
    let frame = encode_frame(message).expect("frame");
    let mut used = 0;
    let handle = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut used);
    assert!(!handle.is_null());
    assert_eq!(used, frame.len());
    if snapshot {
        assert!(pg_msg_snapshot(handle).is_null());
    } else {
        assert!(pg_msg_delta(handle).is_null());
    }
    assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    pg_msg_free(handle);
}

#[test]
fn hostile_sparse_tables_are_rejected_by_both_picture_interfaces() {
    let mut pane = PaneState::new(1, 8, 2, 2);
    pane.feed("e中\r\ne\r\ne中".as_bytes());
    let snapshot = pane.snapshot_message(true);
    let delta = pane.delta_message().expect("delta");
    let oversized = format!("e{}", "\u{301}".repeat(2048));
    for entries in [
        vec![entry(0, 0, "")],
        vec![entry(0, 0, "e\u{301}"), entry(0, 0, "e\u{300}")],
        vec![entry(2, 0, "e\u{301}")],
        vec![entry(i32::MAX, 0, "e\u{301}")],
        vec![entry(i32::MIN, 0, "e\u{301}")],
        vec![entry(-2, 0, "e\u{301}")],
        vec![entry(0, u16::MAX, "e\u{301}")],
        vec![entry(1, 2, " \u{301}")], // Wide continuation.
        vec![entry(0, 0, "x\u{301}")],
        vec![entry(0, 0, "e\0")],
        vec![entry(0, 0, "e\u{85}")],
        vec![entry(0, 0, &oversized)],
    ] {
        reject_picture(&with_text(snapshot.clone(), entries.clone()));
        reject_picture(&with_text(delta.clone(), entries));
    }
    // A history coordinate is legal in a snapshot but never in a delta.
    let history = with_text(snapshot, vec![entry(-1, 0, "e\u{301}")]);
    assert_eq!(
        snapshot_of(&history).unwrap().1.history[0][0].text(),
        "e\u{301}"
    );
    reject_picture(&with_text(delta.clone(), vec![entry(-1, 0, "e\u{301}")]));
    let mut missing_row = delta;
    if let Message::Delta { rows, .. } = &mut missing_row {
        rows.retain(|row| row.y == 1);
    }
    reject_picture(&with_text(missing_row, vec![entry(0, 0, "e\u{301}")]));
}

#[test]
fn malformed_delta_rows_with_text_fail_before_ffi_ownership_transfer() {
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed("e\u{301}".as_bytes());
    let delta = pane.delta_message().expect("delta");
    for width in [0, 9, usize::from(u16::MAX) + 1] {
        let mut message = delta.clone();
        if let Message::Delta { rows, .. } = &mut message {
            let mut bad = rows[0].clone();
            bad.y = 2;
            bad.cells.resize(width, bad.cells[0]);
            rows.push(bad);
        }
        reject_picture(&message);
    }
    let mut empty = delta.clone();
    if let Message::Delta { rows, .. } = &mut empty {
        rows.clear();
    }
    reject_picture(&empty);
    let mut duplicate = delta;
    if let Message::Delta { rows, .. } = &mut duplicate {
        rows.push(rows[0].clone());
    }
    reject_picture(&duplicate);
    assert!(pg_msg_delta(std::ptr::null()).is_null());
    pg_delta_free(std::ptr::null_mut());
    pg_snapshot_free(std::ptr::null_mut());
}

#[test]
fn old_frames_and_empty_tables_clear_replaced_text_without_changing_cell_encoding() {
    use ciborium::value::Value;
    use serde::Deserialize;
    // The pre-extension reader knows only these fields and ignores the text table.
    #[derive(Deserialize)]
    struct LegacySnapshot {
        screen: Vec<Vec<pigeon_core::api::WireCell>>,
    }
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed("e\u{301}".as_bytes());
    let snapshot = pane.snapshot_message(false);
    let payload = pigeon_core::api::encode(&snapshot).unwrap();
    let legacy: LegacySnapshot = ciborium::from_reader(payload.as_slice()).unwrap();
    assert_eq!(legacy.screen[0][0].codepoint, u32::from('e'));
    for message in [snapshot, pane.delta_message().unwrap()] {
        let payload = pigeon_core::api::encode(&message).unwrap();
        let mut value: Value = ciborium::from_reader(payload.as_slice()).unwrap();
        let Value::Map(fields) = &mut value else {
            panic!("map")
        };
        fields.retain(|(key, _)| key.as_text() != Some("text"));
        for empty_table in [false, true] {
            if empty_table {
                fields.push((Value::Text("text".into()), Value::Array(vec![])));
            }
            let mut payload = vec![];
            ciborium::into_writer(&Value::Map(fields.clone()), &mut payload).unwrap();
            let old = pigeon_core::api::decode(&payload).unwrap();
            let cell = match &old {
                Message::Snapshot { .. } => snapshot_of(&old).unwrap().1.screen[0][0].clone(),
                _ => delta_of(&old).unwrap().rows[0].1[0].clone(),
            };
            assert_eq!(cell.text(), "e");
            assert!(cell.extra_text.is_none());
            let mut consumed = 0;
            let frame = encode_frame(&old).unwrap();
            let handle = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
            if matches!(old, Message::Snapshot { .. }) {
                let snapshot = pg_msg_snapshot(handle);
                assert!(!snapshot.is_null());
                // SAFETY: snapshot is live and owned until freed below.
                unsafe {
                    assert_eq!((*snapshot).text_count, 0);
                    assert!((*snapshot).text.is_null());
                }
                pg_snapshot_free(snapshot);
            } else {
                let delta = pg_msg_delta(handle);
                assert!(!delta.is_null());
                // SAFETY: delta is live and owned until freed below.
                unsafe {
                    assert_eq!((*delta).text_count, 0);
                    assert!((*delta).text.is_null());
                }
                pg_delta_free(delta);
            }
            pg_msg_free(handle);
        }
    }
}

#[test]
fn cell_text_limit_is_utf8_safe_and_accepts_the_exact_boundary() {
    let mut pane = PaneState::new(1, 8, 2, 0);
    let text = format!("e{}\u{20dd}", "\u{301}".repeat(2046)); // Exactly 4096 bytes.
    assert_eq!(text.len(), 4096);
    pane.feed(format!("{text}\u{301}").as_bytes());
    let snapshot = pane.snapshot_message(false);
    let (_, picture) = snapshot_of(&snapshot).unwrap();
    assert_eq!(picture.screen[0][0].text(), text);
    let frame = encode_frame(&snapshot).unwrap();
    let (message, _) = decode_frame(&frame).unwrap().unwrap();
    assert_eq!(snapshot_of(&message).unwrap().1.screen[0][0].text(), text);
}

#[test]
fn old_pictures_remain_owned_when_new_rows_overwrite_or_erase_marks() {
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed("e\u{301}".as_bytes());
    let old_snapshot = pane.snapshot_message(false);
    let first_delta = pane.delta_message().unwrap();
    let old_delta = delta_of(&first_delta).unwrap();
    pane.feed(b"\rX");
    let replaced = delta_of(&pane.delta_message().unwrap()).unwrap();
    assert_eq!(replaced.rows[0].1[0].text(), "X");
    assert!(replaced.rows[0].1[0].extra_text.is_none());
    pane.feed("\rX\u{301}\r\x1b[K".as_bytes());
    let erased = delta_of(&pane.delta_message().unwrap()).unwrap();
    assert_eq!(erased.rows[0].1[0].text(), " ");
    assert!(erased.rows[0].1[0].extra_text.is_none());
    assert_eq!(old_delta.rows[0].1[0].text(), "e\u{301}");
    assert_eq!(
        snapshot_of(&old_snapshot).unwrap().1.screen[0][0].text(),
        "e\u{301}"
    );
    let replay = pane.deltas_since(old_delta.seq).unwrap();
    assert_eq!(
        delta_of(replay.last().unwrap()).unwrap().rows[0].1[0].text(),
        " "
    );
}

#[test]
fn zwj_codepoints_survive_but_cross_cell_cluster_width_is_still_unsupported() {
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed("👩\u{200d}💻".as_bytes());
    let (_, snapshot) = snapshot_of(&pane.snapshot_message(false)).unwrap();
    assert_eq!(snapshot.screen[0][0].text(), "👩\u{200d}");
    assert_eq!(snapshot.screen[0][2].text(), "💻");
    assert_eq!(
        snapshot.cursor.x, 4,
        "the engine still allocates two wide cells"
    );
}

#[test]
fn history_budget_counts_text_and_keeps_the_newest_rows() {
    use std::sync::Arc;
    let mut pane = PaneState::new(1, 8, 2, 0);
    let mut picture = pane.snapshot();
    let mut row = vec![pigeon_core::api::Cell::BLANK; 8];
    for cell in &mut row {
        cell.codepoint = u32::from('e');
        cell.extra_text = Some(Arc::from(format!("e{}", "\u{301}".repeat(2047))));
    }
    picture.history = vec![row; 100];
    picture.history.last_mut().unwrap()[0].extra_text = Some(Arc::from("e\u{300}"));
    let message = PaneState::from_snapshot(picture).snapshot_message(true);
    let (_, received) = snapshot_of(&message).unwrap();
    assert!(received.history.len() < 100);
    assert_eq!(received.history.last().unwrap()[0].text(), "e\u{300}");
    let Message::Snapshot { text, .. } = &message else {
        panic!("snapshot")
    };
    let charge = received.history.len() * (8 * 20 + 16)
        + text
            .iter()
            .filter(|entry| entry.row < 0)
            .map(|entry| entry.text.len() + 32)
            .sum::<usize>();
    assert!(charge <= 2 * 1024 * 1024);
    assert!(encode_frame(&message).unwrap().len() < pigeon_core::api::MAX_FRAME);
    assert!(text
        .iter()
        .filter(|entry| entry.row < 0)
        .all(|entry| { entry.row >= -(received.history.len() as i32) }));
}

#[test]
fn backlog_budget_evicts_combining_text_even_with_few_cells() {
    use pigeon_core::session::Backlog;
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed("e\u{301}".as_bytes());
    let mut message = pane.delta_message().unwrap();
    if let Message::Delta { rows, text, .. } = &mut message {
        rows.truncate(1);
        let cell = rows[0].cells[0];
        rows[0].cells.fill(cell);
        *text = (0..8)
            .map(|col| entry(0, col, &format!("e{}", "\u{301}".repeat(2047))))
            .collect();
    }
    let mut backlog = Backlog::default();
    for seq in 1..=400 {
        if let Message::Delta { seq: value, .. } = &mut message {
            *value = seq;
        }
        backlog.push(seq, &message);
    }
    // 3,200 cells alone fit easily; ~13 MiB of strings must not.
    assert!(backlog.since(0, 400).is_none());
    let recent = backlog.since(399, 400).expect("latest frame retained");
    assert_eq!(
        delta_of(&recent[0]).unwrap().rows[0].1[0].text().len(),
        4095
    );
}

#[test]
fn dense_screen_text_obeys_the_existing_frame_limit_without_silent_loss() {
    use std::sync::Arc;
    let mut pane = PaneState::new(1, 64, 40, 0);
    let mut picture = pane.snapshot();
    let text: Arc<str> = Arc::from(format!("e{}", "\u{301}".repeat(2047)));
    for cell in picture.screen.iter_mut().flatten() {
        cell.codepoint = u32::from('e');
        cell.extra_text = Some(Arc::clone(&text));
    }
    let message = PaneState::from_snapshot(picture).snapshot_message(false);
    assert_eq!(
        snapshot_of(&message).unwrap().1.screen[39][63].text(),
        text.as_ref()
    );
    assert_eq!(encode_frame(&message), Err(PgError::Overflow));
}
