#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use pigeon_core::api::{self, Config, Core, GridPoint, GridSelection, SelectionUnit};

fn picture(cols: u16, text: &str) -> api::Snapshot {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(cols, 4).unwrap();
    core.feed(pane, text.as_bytes()).unwrap();
    core.snapshot(pane).unwrap()
}
fn select(
    picture: &api::Snapshot,
    a: (u16, u16),
    b: (u16, u16),
    unit: SelectionUnit,
    rectangle: bool,
) -> api::Selection {
    api::select_grid(
        picture,
        &GridSelection {
            anchor: GridPoint { row: a.0, col: a.1 },
            head: GridPoint { row: b.0, col: b.1 },
            unit,
            rectangle,
        },
    )
    .unwrap()
}
#[test]
fn wide_continuations_and_combining_text_are_copied_whole_in_both_directions() {
    let p = picture(12, "a中e\u{301}Z");
    for (a, b) in [((0, 2), (0, 3)), ((0, 3), (0, 2))] {
        let selected = select(&p, a, b, SelectionUnit::Cell, false);
        assert_eq!(selected.text, "中e\u{301}");
        assert_eq!((selected.spans[0].first, selected.spans[0].end), (1, 4));
    }
}
#[test]
fn linear_copy_joins_soft_wraps_and_keeps_hard_breaks() {
    let p = picture(6, "abcdefghi\r\nend");
    let selected = select(&p, (0, 0), (2, 5), SelectionUnit::Cell, false);
    assert_eq!(selected.text, "abcdefghi\nend");
    assert_eq!(selected.spans.len(), 3);
}
#[test]
fn rectangle_preserves_columns_and_word_line_units_use_core_rules() {
    let p = picture(20, "src/main.rs:12 rest\r\nabc   xyz");
    assert_eq!(
        select(&p, (0, 5), (0, 5), SelectionUnit::Word, false).text,
        "src/main.rs:12"
    );
    assert_eq!(
        select(&p, (1, 4), (1, 4), SelectionUnit::Line, false).text,
        "abc   xyz"
    );
    assert_eq!(
        select(&p, (0, 0), (1, 2), SelectionUnit::Cell, true).text,
        "src\nabc"
    );
    assert_eq!(
        select(&p, (1, 3), (1, 5), SelectionUnit::Cell, true).text,
        "   "
    );
}
#[test]
fn malformed_grid_and_outside_points_are_rejected() {
    let mut p = picture(8, "hello");
    let selection = GridSelection {
        anchor: GridPoint { row: 0, col: 0 },
        head: GridPoint { row: 4, col: 0 },
        unit: SelectionUnit::Cell,
        rectangle: false,
    };
    assert_eq!(
        api::select_grid(&p, &selection),
        Err(api::PgError::InvalidArg)
    );
    p.screen.pop();
    assert_eq!(
        api::select_grid(
            &p,
            &GridSelection {
                head: GridPoint { row: 0, col: 1 },
                ..selection
            }
        ),
        Err(api::PgError::InvalidArg)
    );
}

#[test]
fn selection_text_budget_rejects_instead_of_returning_a_truncated_copy() {
    let mut p = picture(1024, "x");
    let text: std::sync::Arc<str> = std::sync::Arc::from("x".repeat(4096));
    for row in &mut p.screen {
        for cell in row {
            cell.extra_text = Some(text.clone());
        }
    }
    let selection = GridSelection {
        anchor: GridPoint { row: 0, col: 0 },
        head: GridPoint { row: 3, col: 1023 },
        unit: SelectionUnit::Cell,
        rectangle: false,
    };
    assert_eq!(api::select_grid(&p, &selection), Err(api::PgError::Limit));
}
