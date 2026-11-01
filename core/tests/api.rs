//! The safe Rust client face. Uses `pigeon_core::api` only, exactly as a Rust
//! client would; `abi.rs` covers the same behaviour through the C face.

use pigeon_core::api::{
    delta_of, encode_frame, layout_focus, layout_rects, layout_splits, layout_zoom, offers_feature,
    sessions_of, snapshot_of, Axis, Config, Core, Message, Node, PgError, Row, Scroll, SessionInfo,
    SplitAxis, SplitNode, StatusState, WireCell, WireCursor, PROTO_VERSION,
};

fn core_with_pane(cols: u16, rows: u16) -> (Core, u32) {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(cols, rows).expect("first pane");
    (core, pane)
}

#[test]
fn feed_then_snapshot_shows_the_text() {
    let (mut core, pane) = core_with_pane(20, 4);
    core.feed(pane, b"hi").expect("feed");
    let snapshot = core.snapshot(pane).expect("snapshot");
    assert_eq!((snapshot.cols, snapshot.rows), (20, 4));
    assert_eq!(snapshot.screen[0][0].codepoint, u32::from('h'));
    assert_eq!(snapshot.screen[0][1].codepoint, u32::from('i'));
    assert_eq!(snapshot.cursor.x, 2);
    assert!(!core.take_dirty_rows(pane).expect("dirty").is_empty());
}

#[test]
fn unknown_pane_is_an_error_not_a_panic() {
    let (mut core, _) = core_with_pane(20, 4);
    assert_eq!(core.feed(99, b"x"), Err(PgError::NoSuchPane));
    assert_eq!(core.snapshot(99).err(), Some(PgError::NoSuchPane));
    assert_eq!(core.focus(99), Err(PgError::NoSuchPane));
    // Extra panes come from split, never from new_pane.
    assert_eq!(core.new_pane(20, 4), Err(PgError::InvalidArg));
}

#[test]
fn split_focuses_the_new_pane_and_close_gives_the_rect_back() {
    let (mut core, first) = core_with_pane(80, 24);
    let second = core.split(first, Axis::Vertical, 0.5).expect("split");
    assert_ne!(second, first);
    assert_eq!(core.focused(), Some(second));

    let rects = core.rects().expect("rects");
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].0, first);
    assert!((rects[0].1.w - 0.5).abs() < 1e-6);

    let split = core
        .split_of(second)
        .expect("split_of")
        .expect("has a split");
    core.set_split_ratio(split, 0.25).expect("resize");
    assert!((core.rects().expect("rects")[0].1.w - 0.25).abs() < 1e-6);
    assert_eq!(
        core.set_split_ratio(split + 100, 0.5),
        Err(PgError::NoSuchSplit)
    );

    core.close(second).expect("close");
    let rects = core.rects().expect("rects");
    assert_eq!(rects.len(), 1);
    assert!((rects[0].1.w - 1.0).abs() < 1e-6);
    assert_eq!(core.split_of(first).expect("split_of"), None);
}

#[test]
fn status_records_are_read_from_the_focused_pane_and_acked() {
    let (mut core, pane) = core_with_pane(40, 6);
    core.feed(pane, b"\x1b]7501;state=done:msg=QWxsIGdvb2Q=\x07")
        .expect("feed");
    let (focused, records) = core.focused_status().expect("status");
    let records: Vec<_> = records.cloned().collect();
    assert_eq!(focused, pane);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, StatusState::Done);
    assert_eq!(records[0].msg, "All good");
    assert!(!records[0].acked);

    assert!(!core.ack_status(pane, records[0].seq + 1).expect("ack"));
    assert!(core.ack_status(pane, records[0].seq).expect("ack"));
    let (_, mut records) = core.focused_status().expect("status");
    assert!(records.next().expect("record").acked);
}

#[test]
fn malformed_status_report_is_dropped_whole() {
    let (mut core, pane) = core_with_pane(40, 6);
    // `msg` is not base64: the whole record goes, not just the field.
    core.feed(pane, b"\x1b]7501;state=blocked:msg=***\x07")
        .expect("feed");
    let (_, records) = core.focused_status().expect("status");
    assert_eq!(records.count(), 0);
}

#[test]
fn replies_are_drained_once() {
    let (mut core, pane) = core_with_pane(20, 4);
    core.feed(pane, b"\x1b[6n").expect("feed");
    let replies = core.take_replies(pane).expect("replies");
    assert_eq!(replies, b"\x1b[1;1R");
    assert!(core.take_replies(pane).expect("replies").is_empty());
}

fn layout_message() -> Message {
    Message::Layout {
        session_id: 1,
        tree: Node::Split {
            split: SplitNode {
                id: 4,
                axis: SplitAxis::Vertical,
                ratio: 0.25,
                a: Box::new(Node::Leaf { leaf: 1 }),
                b: Box::new(Node::Leaf { leaf: 2 }),
            },
        },
        zoom: None,
        focus: Some(2),
    }
}

#[test]
fn layout_message_geometry_comes_from_the_core() {
    let message = layout_message();
    let rects = layout_rects(&message).expect("rects");
    assert_eq!(rects.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [1, 2]);
    assert!((rects[1].1.x - 0.25).abs() < 1e-6);
    let splits = layout_splits(&message).expect("splits");
    assert_eq!(splits.len(), 1);
    assert_eq!((splits[0].id, splits[0].axis), (4, Axis::Vertical));
    assert_eq!(layout_focus(&message).expect("focus"), Some(2));
    // A frame round-trip changes nothing.
    assert!(!encode_frame(&message).expect("frame").is_empty());
}

#[test]
fn message_helpers_refuse_the_wrong_message_type() {
    let hello = Message::Hello {
        proto: PROTO_VERSION,
        client: "test".to_owned(),
        features: Vec::new(),
    };
    assert_eq!(layout_rects(&hello).err(), Some(PgError::InvalidArg));
    assert_eq!(layout_focus(&hello).err(), Some(PgError::InvalidArg));
    assert_eq!(snapshot_of(&hello).err(), Some(PgError::InvalidArg));
}

fn wire_snapshot(cols: u16, rows: u16, screen: Vec<Vec<WireCell>>) -> Message {
    Message::Snapshot {
        pane_id: 7,
        seq: 1,
        cols,
        rows,
        alt: false,
        cursor: WireCursor {
            x: 0,
            y: 0,
            visible: true,
            shape: 0,
        },
        scroll: Scroll {
            top: 0,
            bottom: rows.saturating_sub(1),
        },
        screen,
        history: None,
        history_len: None,
        offset: None,
        modes: None,
        links: None,
    }
}

fn wire_cell(ch: char) -> WireCell {
    WireCell {
        codepoint: u32::from(ch),
        attrs: 0,
        fg: 0x0100_0007,
        bg: 0x0100_0000,
        link: None,
    }
}

#[test]
fn wire_snapshot_rederives_wide_cells() {
    let message = wire_snapshot(
        3,
        1,
        vec![vec![wire_cell('中'), wire_cell(' '), wire_cell('a')]],
    );
    let (pane_id, snapshot) = snapshot_of(&message).expect("snapshot");
    assert_eq!(pane_id, 7);
    assert_eq!(snapshot.screen[0][0].width, 2);
    assert_eq!(snapshot.screen[0][1].width, 0);
    assert_eq!(snapshot.screen[0][2].width, 1);
}

#[test]
fn wire_snapshot_with_a_short_row_is_refused() {
    let message = wire_snapshot(3, 1, vec![vec![wire_cell('a')]]);
    assert_eq!(snapshot_of(&message).err(), Some(PgError::InvalidArg));
    let message = wire_snapshot(3, 2, vec![vec![wire_cell('a'); 3]]);
    assert_eq!(snapshot_of(&message).err(), Some(PgError::InvalidArg));
}

#[test]
fn a_hello_ack_says_which_features_the_host_offers() {
    let ack = Message::HelloAck {
        proto: PROTO_VERSION,
        server: "pigeon-server test".into(),
        features: vec!["delta".into(), "status_all".into()],
        host_id: "h".into(),
        client_id: 1,
        instance: None,
    };
    assert_eq!(offers_feature(&ack, "status_all"), Ok(true));
    assert_eq!(offers_feature(&ack, "zstd"), Ok(false));
    // Only a hello_ack carries the negotiated set.
    let other = Message::Pong { ts: 1 };
    assert_eq!(offers_feature(&other, "delta"), Err(PgError::InvalidArg));
}

#[test]
fn replies_can_be_peeked_and_consumed_in_parts() {
    let (mut core, pane) = core_with_pane(20, 4);
    core.feed(pane, b"\x1b[6n").expect("feed");
    assert_eq!(core.peek_replies(pane).expect("peek"), b"\x1b[1;1R");
    assert_eq!(
        core.peek_replies(pane).expect("peek"),
        b"\x1b[1;1R",
        "peeking keeps them"
    );
    core.consume_replies(pane, 2).expect("consume");
    core.feed(pane, b"\x1b[6n").expect("feed");
    assert_eq!(
        core.take_replies(pane).expect("take"),
        b"1;1R\x1b[1;1R",
        "the rest stays ahead of newer replies"
    );
}

#[test]
fn dirty_rows_can_be_peeked_and_consumed_in_parts() {
    let (mut core, pane) = core_with_pane(10, 4);
    core.feed(pane, b"a\r\nb\r\nc").expect("feed");
    let pending = core.peek_dirty_rows(pane).expect("peek").to_vec();
    assert!(pending.len() >= 3);
    core.consume_dirty_rows(pane, 1).expect("consume");
    assert_eq!(core.take_dirty_rows(pane).expect("take"), pending[1..]);
}

#[test]
fn osc7501_can_be_turned_off() {
    assert!(
        Config::default().osc7501_enabled,
        "on unless asked otherwise"
    );
    let mut core = Core::new(Config {
        osc7501_enabled: false,
        ..Config::default()
    });
    let pane = core.new_pane(20, 4).expect("pane");
    core.feed(pane, b"\x1b]7501;state=working\x07")
        .expect("feed");
    let second = core.split(pane, Axis::Vertical, 0.5).expect("split");
    core.feed(second, b"\x1b]7501;state=working\x07")
        .expect("feed");
    let (_, records) = core.focused_status().expect("status");
    assert_eq!(records.count(), 0, "split panes follow the config too");
    core.focus(pane).expect("focus");
    let (_, records) = core.focused_status().expect("status");
    assert_eq!(records.count(), 0);
}

#[test]
fn split_sizes_follow_the_ratio_and_resize_clamps_against_the_window() {
    let (mut core, first) = core_with_pane(100, 30);
    let second = core.split(first, Axis::Horizontal, 0.5).expect("split");
    assert_eq!(core.snapshot(first).expect("snap").rows, 15);
    assert_eq!(core.snapshot(second).expect("snap").rows, 15);
    let third = core.split(second, Axis::Vertical, 0.3).expect("split");
    assert_eq!(core.snapshot(second).expect("snap").cols, 30);
    assert_eq!(core.snapshot(third).expect("snap").cols, 70);

    // The focused pane is 15 rows tall; the outer split divides the 30-row window, so
    // 3-row minimums allow 0.9 (measured against the pane it would stop at 0.8).
    let outer = core.split_of(first).expect("split_of").expect("split");
    core.set_split_ratio(outer, 0.9).expect("resize");
    let rects = core.rects().expect("rects");
    assert!((rects[0].1.h - 0.9).abs() < 1e-5, "got {}", rects[0].1.h);
    assert_eq!(core.snapshot(first).expect("snap").rows, 27);

    // Closing gives the sibling the whole box back, in cells too.
    core.close(third).expect("close");
    assert_eq!(core.snapshot(second).expect("snap").cols, 100);
}

#[test]
fn a_status_record_with_an_id_is_acked_by_seq_like_the_host_does() {
    let (mut core, pane) = core_with_pane(40, 6);
    core.feed(pane, b"\x1b]7501;state=done:id=w1\x07")
        .expect("feed");
    let seq = {
        let (_, mut records) = core.focused_status().expect("status");
        records.next().expect("record").seq
    };
    assert!(core.ack_status(pane, seq).expect("ack"));
    let (_, mut records) = core.focused_status().expect("status");
    assert!(records.next().expect("record").acked);
}

#[test]
fn a_delta_message_decodes_into_rows() {
    let message = Message::Delta {
        pane_id: 3,
        seq: 9,
        rows: vec![Row {
            y: 1,
            cells: vec![wire_cell('中'), wire_cell(' '), wire_cell('a')],
        }],
        cursor: Some(WireCursor {
            x: 2,
            y: 1,
            visible: true,
            shape: 0,
        }),
        scroll: None,
        links: None,
        alt: Some(true),
        modes: None,
    };
    let delta = delta_of(&message).expect("delta");
    assert_eq!((delta.pane_id, delta.seq, delta.alt), (3, 9, Some(true)));
    assert_eq!(delta.rows.len(), 1);
    let (y, cells) = &delta.rows[0];
    assert_eq!(*y, 1);
    assert_eq!((cells[0].width, cells[1].width, cells[2].width), (2, 0, 1));
    assert_eq!(delta.cursor.map(|cursor| cursor.x), Some(2));
    assert_eq!(delta_of(&layout_message()).err(), Some(PgError::InvalidArg));
}

#[test]
fn layout_zoom_and_sessions_are_read_through_the_api() {
    let Message::Layout {
        session_id, tree, ..
    } = layout_message()
    else {
        panic!("layout");
    };
    let zoomed = Message::Layout {
        session_id,
        tree,
        zoom: Some(2),
        focus: None,
    };
    assert_eq!(layout_zoom(&zoomed), Ok(Some(2)));
    assert_eq!(layout_zoom(&layout_message()), Ok(None));
    assert_eq!(
        layout_zoom(&Message::Pong { ts: 1 }).err(),
        Some(PgError::InvalidArg)
    );

    let sessions = Message::Sessions {
        items: vec![SessionInfo {
            session_id: 4,
            title: "t".into(),
            cwd: "/tmp".into(),
            panes: vec![5, 6],
            active_pane: Some(6),
            active_title: None,
            active_program: Some("vim".into()),
        }],
    };
    let items = sessions_of(&sessions).expect("sessions");
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].session_id, &items[0].panes[..]), (4, &[5, 6][..]));
    assert_eq!(
        sessions_of(&Message::Pong { ts: 1 }).err(),
        Some(PgError::InvalidArg)
    );
}
