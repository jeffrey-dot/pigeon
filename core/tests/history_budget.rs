#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(feature = "internals")]

use pigeon_core::term::{alacritty::AlacrittyTerm, frozen::FrozenTerm, Cell, Term};

fn charge(row: &[Cell]) -> usize {
    row.len() * 20
        + 16
        + row
            .iter()
            .filter_map(|cell| cell.extra_text.as_ref())
            .map(|text| text.len() + 32)
            .sum::<usize>()
}

#[test]
fn live_and_frozen_history_keep_the_same_complete_newest_suffix() {
    let mut term = AlacrittyTerm::new(8, 2, 100);
    for text in [
        "old",
        "e\u{301}",
        "中\u{fe0f}",
        "\t\r\x1b[C\u{301}",
        "abcdefghijk",
        "last\u{300}",
        "live",
    ] {
        term.feed(format!("{text}\r\n").as_bytes());
    }
    let original = term.snapshot();
    let frozen = FrozenTerm::new(original.clone());
    let newest = charge(original.history.last().expect("history"));
    for budget in [
        0,
        newest - 1,
        newest,
        newest + 1,
        newest * 2,
        2048,
        usize::MAX,
    ] {
        let mut remaining = budget;
        let keep = original
            .history
            .iter()
            .rev()
            .take_while(|row| match remaining.checked_sub(charge(row)) {
                Some(rest) => {
                    remaining = rest;
                    true
                }
                None => false,
            })
            .count();
        for source in [&term as &dyn Term, &frozen as &dyn Term] {
            let snapshot = source.snapshot_with_history_budget(budget);
            assert_eq!(
                snapshot.history,
                original.history[original.history.len() - keep..]
            );
            assert_eq!(snapshot.screen, original.screen);
            assert_eq!(snapshot.cursor, original.cursor);
        }
    }
    assert_eq!(
        term.snapshot(),
        original,
        "export must not trim the engine's scrollback"
    );
}

#[test]
fn a_newest_row_larger_than_the_budget_is_not_partially_exported_or_skipped() {
    let mut term = AlacrittyTerm::new(8, 2, 10);
    term.feed(b"cheap\r\n");
    let text = format!("e{}", "\u{301}".repeat(3000));
    term.feed(format!("{text}\r\nlive\r\n").as_bytes());
    let original = term.snapshot();
    let newest = original.history.last().expect("dense newest row");
    assert_eq!(
        newest[0].text().len(),
        4095,
        "existing UTF-8 export boundary"
    );
    let budget = charge(newest);
    assert!(term
        .snapshot_with_history_budget(budget - 1)
        .history
        .is_empty());
    assert_eq!(
        term.snapshot_with_history_budget(budget).history,
        vec![newest.clone()]
    );
    assert_eq!(term.screen_snapshot().screen, original.screen);
}

#[test]
fn alternate_screen_export_preserves_its_screen_under_a_zero_history_budget() {
    let mut term = AlacrittyTerm::new(8, 2, 10);
    term.feed("old\r\nold\r\nold\x1b[?1049he\u{301}".as_bytes());
    let snapshot = term.snapshot_with_history_budget(0);
    assert!(snapshot.alt);
    assert!(snapshot.history.is_empty());
    assert_eq!(snapshot.screen, term.screen_snapshot().screen);
}
