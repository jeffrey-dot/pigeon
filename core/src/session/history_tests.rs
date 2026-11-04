use super::{PaneState, HISTORY_BUDGET};
use crate::api::snapshot_of;
use crate::term::alacritty::CONVERTED_HISTORY_ROWS;

#[test]
fn dense_history_only_converts_rows_that_will_be_exported() {
    let mut pane = PaneState::new(1, 8, 2, 200);
    let text = format!("e{}", "\u{301}".repeat(2047));
    let line = format!("{}\r\n", text.repeat(8));
    for _ in 0..150 {
        pane.feed(line.as_bytes());
    }
    pane.feed("z\u{300}\r\nlive\u{301}".as_bytes());
    let live = pane.snapshot_message(false);
    CONVERTED_HISTORY_ROWS.set(0);
    let message = pane.snapshot_message(true);
    let converted = CONVERTED_HISTORY_ROWS.get();
    let (_, snapshot) = snapshot_of(&message).expect("valid text snapshot");
    let row_charge = 8 * (20 + text.len() + 32) + 16;
    assert_eq!(snapshot.history.len(), HISTORY_BUDGET / row_charge);
    assert_eq!(snapshot.history.last().expect("newest row")[0].text(), text);
    assert_eq!(
        snapshot.screen,
        snapshot_of(&live).expect("live snapshot").1.screen
    );
    assert_eq!(
        converted,
        snapshot.history.len(),
        "discarded rows must never be converted"
    );
}

#[test]
fn oversized_newest_history_row_is_rejected_before_any_conversion() {
    let mut pane = PaneState::new(1, 512, 2, 10);
    let text = format!("e{}", "\u{301}".repeat(2047));
    pane.feed(b"older cheap row\r\n");
    pane.feed(text.repeat(512).as_bytes());
    pane.feed(b"\r\nlive\r\n");
    let live = pane.snapshot_message(false);
    CONVERTED_HISTORY_ROWS.set(0);
    let message = pane.snapshot_message(true);
    let (_, snapshot) = snapshot_of(&message).expect("valid text snapshot");
    assert!(
        snapshot.history.is_empty(),
        "no partial rows or gaps in history"
    );
    assert_eq!(CONVERTED_HISTORY_ROWS.get(), 0);
    assert_eq!(
        snapshot.screen,
        snapshot_of(&live).expect("live snapshot").1.screen
    );
}
