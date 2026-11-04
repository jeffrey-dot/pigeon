// Tests assert through public APIs, including the raw C ABI.
#![allow(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Text fidelity at the safe client interface, including logical line boundaries.

use pigeon_core::api::{Config, Core, ATTR_WRAP};

#[test]
fn combining_marks_survive_the_safe_snapshot_interface() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(8, 2).expect("pane");
    core.feed(pane, "e\u{301}".as_bytes()).expect("feed");
    let picture = core.snapshot(pane).expect("snapshot");
    assert_eq!(picture.screen[0][0].text(), "e\u{301}");
    assert_eq!(picture.cursor.x, 1, "marks do not advance the cursor");
    core.feed(pane, b"\rX").expect("overwrite");
    assert_eq!(
        core.snapshot(pane).expect("snapshot").screen[0][0].text(),
        "X"
    );
}

#[cfg(feature = "internals")]
#[test]
fn combining_text_round_trips_in_snapshot_delta_and_history() {
    use pigeon_core::api::{decode_frame, delta_of, encode_frame, snapshot_of};
    use pigeon_core::session::PaneState;
    let mut pane = PaneState::new(7, 8, 2, 10);
    pane.feed("e\u{301}".as_bytes());
    let delta = pane.delta_message().expect("delta");
    let frame = encode_frame(&delta).expect("encode");
    let (decoded, _) = decode_frame(&frame).expect("decode").expect("complete");
    assert_eq!(
        delta_of(&decoded).expect("delta").rows[0].1[0].text(),
        "e\u{301}"
    );
    pane.feed(b"\r\nx\r\ny");
    let message = pane.snapshot_message(true);
    let frame = encode_frame(&message).expect("encode");
    let (decoded, _) = decode_frame(&frame).expect("decode").expect("complete");
    assert_eq!(
        snapshot_of(&decoded).expect("snapshot").1.history[0][0].text(),
        "e\u{301}"
    );
    let (scrolled, _) = pane.scroll_message(1);
    assert_eq!(
        snapshot_of(&scrolled).expect("scroll").1.screen[0][0].text(),
        "e\u{301}"
    );
}

#[cfg(feature = "internals")]
#[test]
fn c_abi_text_lives_until_its_snapshot_or_delta_is_freed() {
    use pigeon_core::api::encode_frame;
    use pigeon_core::ffi::*;
    use pigeon_core::session::PaneState;
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed("e\u{301}".as_bytes());
    for message in [
        pane.snapshot_message(false),
        pane.delta_message().expect("delta"),
    ] {
        let frame = encode_frame(&message).expect("frame");
        let mut used = 0;
        let handle = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut used);
        if let pigeon_core::api::Message::Snapshot { .. } = message {
            let snapshot = pg_msg_snapshot(handle);
            pg_msg_free(handle);
            assert!(!snapshot.is_null());
            // SAFETY: the snapshot owns its sparse table and bytes until freed below.
            let text = unsafe { (*(*snapshot).text).text };
            assert_eq!(
                // SAFETY: text is borrowed from the still-live snapshot.
                unsafe { std::slice::from_raw_parts(text.ptr, text.len) },
                "e\u{301}".as_bytes()
            );
            pg_snapshot_free(snapshot);
        } else {
            let delta = pg_msg_delta(handle);
            pg_msg_free(handle);
            assert!(!delta.is_null());
            // SAFETY: the delta owns its sparse table and bytes until freed below.
            let text = unsafe { (*(*delta).text).text };
            assert_eq!(
                // SAFETY: text is borrowed from the still-live delta.
                unsafe { std::slice::from_raw_parts(text.ptr, text.len) },
                "e\u{301}".as_bytes()
            );
            pg_delta_free(delta);
        }
    }
}

#[cfg(feature = "internals")]
#[test]
fn malformed_sparse_text_is_rejected() {
    use pigeon_core::api::{snapshot_of, CellText, Message, PgError};
    use pigeon_core::session::PaneState;
    let mut pane = PaneState::new(1, 8, 2, 0);
    pane.feed(b"e");
    for entry in [
        CellText {
            row: 2,
            col: 0,
            text: "e\u{301}".into(),
        },
        CellText {
            row: -1,
            col: 0,
            text: "e\u{301}".into(),
        },
        CellText {
            row: 0,
            col: 8,
            text: "e\u{301}".into(),
        },
        CellText {
            row: 0,
            col: 0,
            text: "x\u{301}".into(),
        },
        CellText {
            row: 0,
            col: 0,
            text: "e\n".into(),
        },
        CellText {
            row: 0,
            col: 0,
            text: format!("e{}", "\u{301}".repeat(4096)),
        },
    ] {
        let mut message = pane.snapshot_message(false);
        if let Message::Snapshot { text, .. } = &mut message {
            text.push(entry);
        }
        assert_eq!(snapshot_of(&message).err(), Some(PgError::InvalidArg));
    }
    let mut message = pane.snapshot_message(false);
    if let Message::Snapshot { text, .. } = &mut message {
        *text = vec![
            CellText {
                row: 0,
                col: 0,
                text: "e\u{301}".into()
            };
            2
        ];
    }
    assert_eq!(snapshot_of(&message).err(), Some(PgError::InvalidArg));
}

#[test]
fn automatic_wrap_is_distinct_from_an_explicit_newline() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(4, 3).expect("pane");
    core.feed(pane, b"abcdefgh\r\nxy").expect("feed");
    let screen = core.snapshot(pane).expect("snapshot");
    assert_ne!(
        screen.screen[0][3].attrs & ATTR_WRAP,
        0,
        "abcd continues into efgh"
    );
    assert_eq!(
        screen.screen[1][3].attrs & ATTR_WRAP,
        0,
        "the CRLF ends the logical line"
    );
}

#[cfg(feature = "internals")]
#[test]
fn wrap_metadata_survives_snapshot_delta_history_and_c_abi() {
    use pigeon_core::api::{decode_frame, delta_of, encode_frame, snapshot_of};
    use pigeon_core::ffi::{pg_msg_decode_frame, pg_msg_free, pg_msg_snapshot, pg_snapshot_free};
    use pigeon_core::session::PaneState;

    let mut pane = PaneState::new(7, 4, 3, 20);
    pane.feed(b"abcdefgh");
    let delta = pane.delta_message().expect("delta");
    let frame = encode_frame(&delta).expect("encode delta");
    let (decoded, _) = decode_frame(&frame).expect("decode").expect("complete");
    let decoded = delta_of(&decoded).expect("delta picture");
    assert_ne!(decoded.rows[0].1[3].attrs & ATTR_WRAP, 0);

    let snapshot = pane.snapshot_message(false);
    let frame = encode_frame(&snapshot).expect("encode snapshot");
    let (decoded, _) = decode_frame(&frame).expect("decode").expect("complete");
    assert_ne!(
        snapshot_of(&decoded).expect("picture").1.screen[0][3].attrs & ATTR_WRAP,
        0
    );
    let mut used = 0;
    let message = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut used);
    let picture = pg_msg_snapshot(message);
    assert!(!picture.is_null());
    assert_ne!(
        // SAFETY: the live snapshot owns a 4-column screen and is freed after this read.
        unsafe { (*(*picture).screen).cells.add(3).read().attrs } & ATTR_WRAP,
        0
    );
    pg_snapshot_free(picture);
    pg_msg_free(message);

    pane.feed(b"\r\nx\r\ny\r\nz");
    let (scrolled, _) = pane.scroll_message(3);
    let (_, history) = snapshot_of(&scrolled).expect("scrolled picture");
    assert_ne!(history.screen[0][3].attrs & ATTR_WRAP, 0);
}
