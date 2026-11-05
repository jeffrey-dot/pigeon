#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(feature = "internals")]

use pigeon_core::{
    api::{delta_of, Message},
    session::{Backlog, PaneState},
    term::{alacritty::AlacrittyTerm, frozen::FrozenTerm, Term},
};

#[test]
fn live_and_frozen_history_account_for_uri_bytes() {
    let mut term = AlacrittyTerm::new(4, 2, 10);
    term.feed("\x1b]8;;https://example.com/owned\x07e\u{301}\x1b]8;;\x07\r\nlive\r\n".as_bytes());
    let original = term.snapshot();
    let newest = original.history.last().unwrap();
    let charge = newest.len() * 20
        + 16
        + newest
            .iter()
            .flat_map(|cell| {
                [cell.extra_text.as_ref(), cell.hyperlink.as_ref()]
                    .into_iter()
                    .flatten()
            })
            .map(|text| text.len() + 32)
            .sum::<usize>();
    let frozen = FrozenTerm::new(original.clone());
    for source in [&term as &dyn Term, &frozen as &dyn Term] {
        assert!(source
            .snapshot_with_history_budget(charge - 1)
            .history
            .is_empty());
        assert_eq!(
            source.snapshot_with_history_budget(charge).history,
            vec![newest.clone()]
        );
    }
}

#[test]
fn backlog_counts_uri_tables_even_when_cell_count_is_small() {
    let mut pane = PaneState::new(1, 128, 1, 0);
    for index in 0..128 {
        pane.feed(
            format!(
                "\x1b]8;;https://example.com/{index}/{}\x07x",
                "a".repeat(1900)
            )
            .as_bytes(),
        );
    }
    let mut message = pane.delta_message().unwrap();
    let mut backlog = Backlog::default();
    for seq in 1..=50 {
        if let Message::Delta { seq: value, .. } = &mut message {
            *value = seq;
        }
        backlog.push(seq, &message);
    }
    assert!(backlog.since(0, 50).is_none());
    let recent = backlog.since(49, 50).unwrap();
    assert_eq!(
        delta_of(&recent[0]).unwrap().rows[0]
            .1
            .iter()
            .filter(|cell| cell.hyperlink.is_some())
            .count(),
        128
    );
}
