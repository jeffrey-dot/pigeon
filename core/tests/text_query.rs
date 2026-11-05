#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pigeon_core::api::{
    self, Config, Core, PgError, TextInfo, TextPoint, TextQuery, TextRange, TextResult,
};

fn core(cols: u16, rows: u16, text: &str) -> (Core, u32) {
    let mut core = Core::new(Config {
        scrollback_lines: 100_000,
        ..Config::default()
    });
    let pane = core.new_pane(cols, rows).unwrap();
    core.feed(pane, text.as_bytes()).unwrap();
    (core, pane)
}
fn search(pattern: &str) -> TextQuery {
    TextQuery::Search {
        generation: 0,
        pattern: pattern.into(),
        case_sensitive: true,
        max_results: 128,
        start: None,
    }
}
fn hits(
    core: &Core,
    pane: u32,
    query: &TextQuery,
) -> (TextInfo, Vec<TextRange>, Option<TextPoint>) {
    let TextResult::Search { info, hits, next } = core.text_query(pane, query).unwrap() else {
        panic!()
    };
    (info, hits, next)
}
fn extract(core: &Core, pane: u32, generation: u64, range: TextRange) -> String {
    let TextResult::Range { text, next, .. } = core
        .text_query(pane, &TextQuery::Range { generation, range })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(next, None);
    text
}

#[test]
fn history_soft_and_hard_breaks_preserve_wide_and_combining_text() {
    let (core, pane) = core(6, 2, "abc你好e\u{301}Z\r\nhard\r\nend");
    let (info, found, _) = hits(&core, pane, &search("abc你好e\u{301}Z\nhard"));
    assert_eq!(found.len(), 1);
    assert!(found[0].start.row < 0);
    assert_eq!(
        extract(&core, pane, info.generation, found[0]),
        "abc你好e\u{301}Z\nhard"
    );
    // Search may begin inside a combining cell without losing its cell coordinate.
    let (_, accent, _) = hits(&core, pane, &search("\u{301}"));
    assert_eq!(accent.len(), 1);
    assert_eq!(accent[0].start.byte, 1);
    assert_eq!(extract(&core, pane, info.generation, accent[0]), "\u{301}");
}

#[test]
fn leading_wide_spacer_does_not_add_a_space() {
    let (core, pane) = core(5, 3, "abcd你X");
    let (info, found, _) = hits(&core, pane, &search("abcd你X"));
    assert_eq!(found.len(), 1);
    assert_eq!(extract(&core, pane, info.generation, found[0]), "abcd你X");
}

#[test]
fn case_rule_is_ascii_only_without_normalization() {
    let (core, pane) = core(40, 3, "AbC É é e\u{301}");
    assert!(hits(&core, pane, &search("abc")).1.is_empty());
    let mut query = search("abc");
    if let TextQuery::Search { case_sensitive, .. } = &mut query {
        *case_sensitive = false;
    }
    assert_eq!(hits(&core, pane, &query).1.len(), 1);
    if let TextQuery::Search { pattern, .. } = &mut query {
        *pattern = "é".into();
    }
    assert_eq!(hits(&core, pane, &query).1.len(), 1);
    assert_eq!(hits(&core, pane, &search("e\u{301}")).1.len(), 1);
}

#[test]
fn stale_is_independent_of_delta_flush_and_invalidates_resize_alt_and_eviction() {
    let (mut core, pane) = core(20, 2, "old");
    let (info, found, _) = hits(&core, pane, &search("old"));
    let query = TextQuery::Range {
        generation: info.generation,
        range: found[0],
    };
    core.feed(pane, b"new").unwrap();
    assert_eq!(core.text_query(pane, &query), Err(PgError::StaleText));
    let generation = core
        .text_query(pane, &TextQuery::Info)
        .unwrap()
        .info()
        .generation;
    core.resize(pane, 10, 2).unwrap();
    assert_eq!(
        core.text_query(
            pane,
            &TextQuery::Range {
                generation,
                range: found[0]
            }
        ),
        Err(PgError::StaleText)
    );
    let generation = core
        .text_query(pane, &TextQuery::Info)
        .unwrap()
        .info()
        .generation;
    core.feed(pane, b"\x1b[?1049hALT").unwrap();
    assert_eq!(
        core.text_query(
            pane,
            &TextQuery::Range {
                generation,
                range: found[0]
            }
        ),
        Err(PgError::StaleText)
    );
    let info = core.text_query(pane, &TextQuery::Info).unwrap().info();
    assert!(info.alt);
    assert_eq!(info.first_row, 0);
    assert!(hits(&core, pane, &search("old")).1.is_empty());
    assert_eq!(hits(&core, pane, &search("ALT")).1.len(), 1);
}

#[cfg(feature = "internals")]
#[test]
fn sync_flush_and_history_eviction_invalidate_queries_before_screen_seq_changes() {
    use pigeon_core::session::PaneState;
    let mut pane = PaneState::new(1, 10, 2, 2);
    pane.feed(b"one\r\ntwo\r\nthree\r\nfour");
    let before = pane.text_query(&TextQuery::Info).unwrap().info();
    pane.feed(b"\x1b[?2026hqueued");
    let pending = pane.text_query(&TextQuery::Info).unwrap().info();
    assert!(pending.generation > before.generation);
    pane.flush_sync();
    let flushed = pane.text_query(&TextQuery::Info).unwrap().info();
    assert!(flushed.generation > pending.generation);
    pane.feed(b"\r\nfive\r\nsix\r\nseven");
    let mut query = search("one");
    if let TextQuery::Search { generation, .. } = &mut query {
        *generation = before.generation;
    }
    assert_eq!(pane.text_query(&query), Err(PgError::StaleText));
}

#[test]
fn result_paging_preserves_overlaps_and_can_be_abandoned() {
    let (core, pane) = core(20, 2, "aaaaa");
    let mut query = search("aaa");
    if let TextQuery::Search { max_results, .. } = &mut query {
        *max_results = 1;
    }
    let mut found = Vec::new();
    loop {
        let (info, page, next) = hits(&core, pane, &query);
        found.extend(page);
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
    assert_eq!(found.len(), 3);
    assert_eq!(
        found
            .iter()
            .map(|range| range.start.col)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(hits(&core, pane, &search("aaaaa")).1.len(), 1);
}

#[test]
fn scan_budget_pages_no_match_and_matches_crossing_a_page() {
    let text = format!(
        "{}START\r\nEND\r\n{}",
        "line\r\n".repeat(127),
        "tail\r\n".repeat(200)
    );
    let (core, pane) = core(10, 4, &text);
    let mut query = search("START\nEND");
    let mut all = Vec::new();
    let mut pages = 0;
    loop {
        let (info, page, next) = hits(&core, pane, &query);
        all.extend(page);
        pages += 1;
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
        assert!(pages < 10);
    }
    assert!(pages > 2);
    assert_eq!(all.len(), 1);
    let (_, none, next) = hits(&core, pane, &search("missing"));
    assert!(none.is_empty());
    assert!(next.is_some());
}

#[test]
fn range_paging_reassembles_logical_text() {
    let input = "你好e\u{301}\r\n".repeat(300);
    let (core, pane) = core(10, 2, &input);
    let info = core.text_query(pane, &TextQuery::Info).unwrap().info();
    let mut range = TextRange {
        start: TextPoint {
            row: info.first_row,
            col: 0,
            byte: 0,
        },
        end: TextPoint {
            row: info.last_row,
            col: info.cols,
            byte: 0,
        },
    };
    let mut output = String::new();
    let mut pages = 0;
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
        output.push_str(&text);
        pages += 1;
        let Some(next) = next else {
            break;
        };
        range.start = next;
        assert!(pages < 10);
    }
    assert!(pages > 1);
    assert_eq!(output, input.replace("\r\n", "\n"));
}

#[test]
fn rejects_invalid_patterns_ranges_utf8_boundaries_and_missing_panes() {
    let (core, pane) = core(10, 2, "你e\u{301}");
    for pattern in ["", "\u{1b}", "\r", &"a".repeat(257)] {
        assert_eq!(
            core.text_query(pane, &search(pattern)),
            Err(PgError::InvalidArg)
        );
    }
    assert_eq!(
        core.text_query(999, &TextQuery::Info),
        Err(PgError::NoSuchPane)
    );
    let info = core.text_query(pane, &TextQuery::Info).unwrap().info();
    for point in [
        TextPoint {
            row: -100,
            col: 0,
            byte: 0,
        },
        TextPoint {
            row: 0,
            col: 11,
            byte: 0,
        },
        TextPoint {
            row: 0,
            col: 0,
            byte: 1,
        },
        TextPoint {
            row: 0,
            col: 1,
            byte: 1,
        },
    ] {
        assert_eq!(
            core.text_query(
                pane,
                &TextQuery::Range {
                    generation: info.generation,
                    range: TextRange {
                        start: point,
                        end: point
                    }
                }
            ),
            Err(PgError::InvalidArg)
        );
    }
    let mut query = search("x");
    if let TextQuery::Search { max_results, .. } = &mut query {
        *max_results = 129;
    }
    assert_eq!(core.text_query(pane, &query), Err(PgError::InvalidArg));
}

#[test]
fn protocol_roundtrip_and_malformed_result() {
    let (core, pane) = core(10, 2, "你好");
    let request = api::text_query_message(42, pane, search("你好")).unwrap();
    assert_eq!(
        api::decode(&api::encode(&request).unwrap()).unwrap(),
        request
    );
    let result = core.text_query(pane, &search("你好")).unwrap();
    let reply = api::Message::TextQueryResult {
        id: 42,
        pane_id: pane,
        result,
    };
    assert_eq!(api::decode(&api::encode(&reply).unwrap()).unwrap(), reply);
    assert_eq!(api::text_result_of(&request), Err(PgError::InvalidArg));
    let mut bad = reply;
    if let api::Message::TextQueryResult {
        result: TextResult::Search { hits, .. },
        ..
    } = &mut bad
    {
        hits[0].end.row = 999;
    }
    assert_eq!(api::encode(&bad), Err(PgError::InvalidArg));
    let mut raw = Vec::new();
    ciborium::into_writer(&bad, &mut raw).unwrap();
    assert_eq!(api::decode(&raw), Err(PgError::InvalidArg));
}

#[path = "text_query/budget.rs"]
mod budget;

#[cfg(feature = "internals")]
#[test]
fn frozen_partial_snapshots_do_not_masquerade_as_complete_query_sources() {
    use pigeon_core::session::PaneState;
    let mut pane = PaneState::new(1, 5, 2, 100);
    pane.feed("abcd你".as_bytes());
    let frozen = PaneState::from_snapshot(pane.snapshot());
    assert_eq!(
        frozen.text_query(&TextQuery::Info),
        Err(PgError::Unsupported)
    );
}
