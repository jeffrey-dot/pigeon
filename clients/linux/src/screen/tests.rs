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

fn snapshot_message() -> Message {
    Message::Snapshot {
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
    }
}

fn screen() -> Screen {
    Screen::from_message(&snapshot_message()).expect("screen")
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
fn history_draw_does_not_authorize_new_live_screen_before_draw() {
    let mut screen = screen();
    screen.scroll.request(-10);
    let mut history = snapshot_message();
    if let Message::Snapshot {
        history_len,
        offset,
        ..
    } = &mut history
    {
        *history_len = Some(20);
        *offset = Some(10);
    }
    assert!(screen.apply_snapshot(&history));
    let drawn = screen.revision;
    screen.scroll.request(0);
    if let Message::Snapshot { offset, .. } = &mut history {
        *offset = Some(0);
    }
    assert!(screen.apply_snapshot(&history));
    assert!(!screen.scroll.active());
    assert_ne!(drawn, screen.revision);
    let live = screen.revision;
    assert!(screen.apply_snapshot(&history));
    assert_eq!(
        screen.revision, live,
        "ignored duplicate cannot certify new content"
    );
}

#[test]
fn search_picture_survives_live_deltas_until_recovery_snapshot() {
    let mut screen = screen();
    let frozen = screen.snapshot.clone();
    screen.search_picture = Some(frozen.clone());
    assert!(screen.apply_delta(&delta(5)));
    assert_eq!(screen.picture(), &frozen);
    screen.search_returning = true;
    assert!(screen.apply_delta(&delta(6)));
    assert!(screen.search_picture.is_some());
    let mut live = snapshot_message();
    if let Message::Snapshot { seq, .. } = &mut live {
        *seq = 6;
    }
    assert!(screen.apply_snapshot(&live));
    assert!(screen.search_picture.is_none());
    assert!(!screen.search_returning);
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

#[test]
fn screen_copy_preserves_cell_text_and_soft_wrap_but_excludes_history() {
    let mut screen = screen();
    screen.snapshot.screen[0][0].extra_text = Some("e\u{301}".into());
    screen.snapshot.screen[0][1].codepoint = '中' as u32;
    screen.snapshot.screen[0][1].width = 2;
    screen.snapshot.screen[0][2].width = 0;
    screen.snapshot.screen[0][2].attrs = pigeon_core::api::ATTR_WRAP;
    screen.snapshot.screen[1][1].codepoint = ' ' as u32;
    screen.snapshot.screen[1][2].codepoint = ' ' as u32;
    screen.snapshot.history = vec![vec![pigeon_core::api::Cell::BLANK; 3]];
    assert_eq!(screen.visible_text(), "e\u{301}中b");
}

#[test]
fn screen_copy_keeps_hard_newlines_and_empty_rows() {
    let mut screen = screen();
    screen.snapshot.screen[1] = vec![pigeon_core::api::Cell::BLANK; 3];
    assert_eq!(screen.visible_text(), "aaa\n");
    screen.snapshot.screen.clear();
    assert_eq!(screen.visible_text(), "");
}

#[test]
fn live_delta_cannot_tear_a_history_snapshot() {
    let mut message = snapshot_message();
    if let Message::Snapshot {
        history_len,
        offset,
        ..
    } = &mut message
    {
        *history_len = Some(100);
        *offset = Some(20);
    }
    let mut screen = Screen::from_message(&message).expect("history screen");
    let before = screen.snapshot.clone();
    assert!(screen.apply_delta(&delta(5)));
    assert_eq!(
        screen.snapshot, before,
        "history must remain a complete host slice"
    );
}

fn scroll_reply(seq_value: u32, position: u32, marker: char) -> Message {
    let mut message = snapshot_message();
    if let Message::Snapshot {
        seq,
        history_len,
        offset,
        screen,
        ..
    } = &mut message
    {
        *seq = seq_value;
        *history_len = Some(100);
        *offset = Some(position);
        *screen = vec![vec![cell(marker); 3]; 2];
    }
    message
}

#[test]
fn rapid_requests_ignore_intermediate_and_live_snapshots_then_resume_delta_seq() {
    let mut screen = screen();
    screen.scroll.request(-10);
    screen.scroll.request(-10);
    let before = screen.snapshot.clone();
    assert!(screen.apply_delta(&delta(20)));
    assert!(screen.apply_snapshot(&scroll_reply(21, 10, 'x')));
    assert_eq!(screen.snapshot, before);
    assert!(screen.apply_snapshot(&snapshot_message()));
    assert_eq!(screen.snapshot, before);
    assert!(screen.apply_snapshot(&scroll_reply(22, 20, 'h')));
    assert_eq!(screen.visible_text(), "hhh\nhhh");
    assert_eq!(screen.scroll.offset, 20);
    screen.scroll.request(0);
    assert!(screen.apply_delta(&delta(25)));
    assert_eq!(screen.visible_text(), "hhh\nhhh");
    assert!(screen.apply_snapshot(&scroll_reply(25, 0, 'l')));
    assert!(screen.apply_delta(&delta(26)));
    assert_eq!(screen.snapshot.screen[0][0].codepoint, u32::from('l'));
    assert_eq!(screen.snapshot.screen[1][2].codepoint, u32::from('x'));
    assert!(!screen.scroll.active());
    assert!(
        !screen.apply_delta(&delta(28)),
        "normal seq validation resumes"
    );
}

#[test]
fn resize_and_exit_do_not_lose_pending_scroll_or_exit_state() {
    let mut screen = screen();
    screen.exited = Some(0);
    screen.scroll.request(-10);
    screen.scroll.request(0);
    assert!(screen.apply_snapshot(&scroll_reply(4, 10, 'h')));
    let mut resized = scroll_reply(4, 0, 'r');
    if let Message::Snapshot { cols, screen, .. } = &mut resized {
        *cols = 4;
        *screen = vec![vec![cell('r'); 4]; 2];
    }
    assert!(screen.apply_snapshot(&resized));
    assert_eq!(screen.cols(), 4);
    assert_eq!(screen.exited, Some(0));
    assert!(!screen.scroll.active());
}

#[test]
fn malformed_scroll_metadata_cannot_consume_the_pending_answer() {
    let mut screen = screen();
    screen.scroll.request(-10);
    assert!(!screen.apply_snapshot(&scroll_reply(5, 101, 'x')));
    let mut half_metadata = scroll_reply(5, 10, 'x');
    if let Message::Snapshot { history_len, .. } = &mut half_metadata {
        *history_len = None;
    }
    assert!(!screen.apply_snapshot(&half_metadata));
    assert!(screen.scroll.active());
    assert!(screen.apply_snapshot(&scroll_reply(6, 10, 'h')));
    assert_eq!(screen.scroll.offset, 10);
}

#[test]
fn late_live_snapshot_cannot_roll_back_a_resumed_delta_base() {
    let mut screen = screen();
    screen.scroll.request(-10);
    assert!(screen.apply_snapshot(&scroll_reply(10, 10, 'h')));
    screen.scroll.request(0);
    assert!(screen.apply_snapshot(&scroll_reply(20, 0, 'l')));
    assert!(screen.apply_delta(&delta(21)));
    let before = screen.snapshot.clone();
    assert!(screen.apply_snapshot(&snapshot_message()));
    assert_eq!(screen.snapshot, before);
    assert!(screen.apply_delta(&delta(22)));
}
