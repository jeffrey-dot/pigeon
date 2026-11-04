use super::*;
use pigeon_core::api::{Row, Scroll, WireCell, WireCursor};

fn cell(ch: char) -> WireCell {
    WireCell {
        codepoint: u32::from(ch),
        attrs: 0,
        fg: 7,
        bg: 0,
        link: None,
    }
}

fn screen() -> Screen {
    Screen::from_message(&Message::Snapshot {
        text: Vec::new(),
        pane_id: 7,
        seq: 4,
        cols: 3,
        rows: 2,
        alt: false,
        cursor: WireCursor {
            x: 0,
            y: 0,
            visible: true,
            shape: 0,
        },
        scroll: Scroll { top: 0, bottom: 1 },
        screen: vec![vec![cell('a'); 3], vec![cell('b'); 3]],
        history: None,
        history_len: None,
        offset: None,
        modes: Some(0),
        links: None,
    })
    .expect("screen")
}

fn delta(seq: u32) -> Message {
    Message::Delta {
        text: Vec::new(),
        pane_id: 7,
        seq,
        rows: vec![Row {
            y: 1,
            cells: vec![cell('中'), cell(' '), cell('x')],
        }],
        cursor: Some(WireCursor {
            x: 2,
            y: 1,
            visible: true,
            shape: 2,
        }),
        scroll: None,
        links: None,
        alt: Some(true),
        modes: Some(1),
    }
}

#[test]
fn a_contiguous_delta_updates_only_its_rows_and_mode_state() {
    let mut screen = screen();
    assert!(screen.apply_delta(&delta(5)));
    assert_eq!(screen.snapshot.screen[0][0].codepoint, u32::from('a'));
    assert_eq!(
        screen.snapshot.screen[1]
            .iter()
            .map(|cell| cell.width)
            .collect::<Vec<_>>(),
        [2, 0, 1]
    );
    assert_eq!(screen.snapshot.cursor.x, 2);
    assert!(screen.snapshot.alt);
    assert!(screen.app_cursor());
    assert!(screen.apply_delta(&delta(5)), "duplicate frame is harmless");
}

#[test]
fn missing_or_malformed_frames_leave_the_previous_screen_intact() {
    let mut screen = screen();
    let before = screen.snapshot.clone();
    assert!(
        !screen.apply_delta(&delta(6)),
        "missing seq 5 requires a snapshot"
    );
    assert_eq!(screen.snapshot, before);
    let mut malformed = delta(5);
    if let Message::Delta { rows, .. } = &mut malformed {
        rows.push(Row {
            y: 2,
            cells: vec![cell('z'); 3],
        });
    }
    assert!(!screen.apply_delta(&malformed), "row is outside the grid");
    assert_eq!(
        screen.snapshot, before,
        "validation happens before any patch"
    );
    assert!(
        screen.apply_delta(&delta(5)),
        "rejected frames did not advance seq"
    );
}

#[test]
fn a_later_plain_row_clears_the_previous_combining_text() {
    let mut screen = screen();
    let mut marked = delta(5);
    if let Message::Delta { text, .. } = &mut marked {
        text.push(pigeon_core::api::CellText {
            row: 1,
            col: 2,
            text: "x\u{301}".into(),
        });
    }
    assert!(screen.apply_delta(&marked));
    assert_eq!(screen.cell(2, 1).expect("cell").text(), "x\u{301}");
    assert!(screen.apply_delta(&delta(6)));
    assert_eq!(screen.cell(2, 1).expect("cell").text(), "x");
}
