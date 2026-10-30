//! The safe Rust client face. Uses `pigeon_core::api` only, exactly as a Rust
//! client would; `abi.rs` covers the same behaviour through the C face.

use pigeon_core::api::{
    encode_frame, layout_focus, layout_rects, layout_splits, snapshot_of, Axis, Config, Core,
    Message, Node, PgError, Scroll, SplitAxis, SplitNode, StatusState, WireCell, WireCursor,
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
        proto: pigeon_core::api::PROTO_VERSION,
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
