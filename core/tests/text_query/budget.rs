use super::*;

#[test]
fn hundred_thousand_line_search_returns_a_bounded_first_page() {
    let (mut core, pane) = core(8, 2, "");
    core.feed(pane, "line\r\n".repeat(100_010).as_bytes())
        .unwrap();
    let (info, found, next) = hits(&core, pane, &search("missing"));
    assert_eq!(info.first_row, -100_000);
    assert!(found.is_empty());
    let next = next.expect("remaining history");
    assert!(next.row <= info.first_row + 128);
    assert!(next.row > info.first_row);
}

#[test]
fn dense_combining_cells_respect_byte_budget_and_keep_matches_across_pages() {
    let cell = format!("e{}", "\u{301}".repeat(2047));
    let input = cell.repeat(40);
    let (core, pane) = core(80, 2, &input);
    let info = core.text_query(pane, &TextQuery::Info).unwrap().info();
    let mut range = TextRange {
        start: TextPoint::default(),
        end: TextPoint {
            row: 0,
            col: 40,
            byte: 0,
        },
    };
    let mut joined = String::new();
    loop {
        let TextResult::Range { text, next, .. } = core
            .text_query(
                pane,
                &TextQuery::Range {
                    generation: info.generation,
                    range,
                },
            )
            .unwrap()
        else {
            panic!()
        };
        assert!(text.len() <= api::MAX_TEXT_BYTES);
        joined.push_str(&text);
        let Some(next) = next else {
            break;
        };
        range.start = next;
    }
    assert_eq!(joined, input);
    let mut query = search("\u{301}e\u{301}");
    let mut count = 0;
    loop {
        let (info, found, next) = hits(&core, pane, &query);
        count += found.len();
        let Some(next) = next else {
            break;
        };
        if let TextQuery::Search {
            generation, start, ..
        } = &mut query
        {
            *generation = info.generation;
            *start = Some(next);
        }
    }
    assert_eq!(count, 39);
}

#[test]
fn empty_rows_cannot_turn_one_page_into_an_unbounded_scan() {
    let (core, pane) = core(10, 2, &"\r\n".repeat(1000));
    let (info, _, next) = hits(&core, pane, &search("z"));
    assert!(next.unwrap().row <= info.first_row + 128);
    // A literal spanning more rows than a page cannot make progress: explicit limit.
    let mut query = search(&"\n".repeat(256));
    let (info, _, next) = hits(&core, pane, &query);
    if let TextQuery::Search {
        generation, start, ..
    } = &mut query
    {
        *generation = info.generation;
        *start = next;
    }
    assert_eq!(core.text_query(pane, &query), Err(PgError::Limit));
}
