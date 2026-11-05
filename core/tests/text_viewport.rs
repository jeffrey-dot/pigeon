#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use pigeon_core::api::{self, Config, Core, PgError, TextQuery, TextResult};

fn fixture() -> (Core, u32, TextQuery) {
    let mut core = Core::new(Config {
        scrollback_lines: 4,
        ..Config::default()
    });
    let pane = core.new_pane(12, 2).unwrap();
    core.feed(pane, "old你é\r\nnext\r\nlast".as_bytes())
        .unwrap();
    let TextResult::Search { info, hits, .. } = core
        .text_query(
            pane,
            &TextQuery::Search {
                generation: 0,
                pattern: "你é".into(),
                case_sensitive: true,
                max_results: 8,
                start: None,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    (
        core,
        pane,
        TextQuery::Viewport {
            generation: info.generation,
            range: hits[0],
        },
    )
}

#[test]
fn viewport_atomically_locates_history_and_roundtrips_without_history_transfer() {
    let (core, pane, query) = fixture();
    let result = core.text_query(pane, &query).unwrap();
    let TextResult::Viewport {
        info,
        range,
        viewport,
    } = &result
    else {
        panic!()
    };
    assert_eq!(range.start.row, -1);
    assert_eq!(viewport.offset, 1);
    let picture = api::text_viewport_snapshot(&result).unwrap();
    assert_eq!(picture.screen[0][3].codepoint, '你' as u32);
    assert_eq!(picture.screen[0][5].text(), "é");
    assert!(picture.history.is_empty());
    assert_eq!(picture.cols, info.cols);
    let message = api::Message::TextQueryResult {
        id: 41,
        pane_id: pane,
        result,
    };
    assert_eq!(
        api::decode_frame(&api::encode_frame(&message).unwrap())
            .unwrap()
            .unwrap()
            .0,
        message
    );
}

#[test]
fn viewport_rejects_output_pending_resize_alt_and_eviction() {
    for change in [
        b"new".as_slice(),
        b"\x1b[?2026hnew",
        b"\x1b[?1049h",
        b"\r\n1\r\n2\r\n3\r\n4\r\n5",
    ] {
        let (mut core, pane, query) = fixture();
        core.feed(pane, change).unwrap();
        assert_eq!(core.text_query(pane, &query), Err(PgError::StaleText));
    }
    let (mut core, pane, query) = fixture();
    core.resize(pane, 8, 3).unwrap();
    assert_eq!(core.text_query(pane, &query), Err(PgError::StaleText));
}

#[test]
fn viewport_rejects_invalid_coordinates_and_malformed_picture() {
    let (core, pane, mut query) = fixture();
    if let TextQuery::Viewport { range, .. } = &mut query {
        range.end.col = 99;
    }
    assert_eq!(core.text_query(pane, &query), Err(PgError::InvalidArg));
    let (core, pane, query) = fixture();
    let mut result = core.text_query(pane, &query).unwrap();
    if let TextResult::Viewport { viewport, .. } = &mut result {
        viewport.offset = 999;
    }
    assert!(api::encode_frame(&api::Message::TextQueryResult {
        id: 41,
        pane_id: pane,
        result
    })
    .is_err());
}

#[test]
fn viewport_rejects_malformed_wire_snapshot_and_zero_generation() {
    let (core, pane, query) = fixture();
    let mut result = core.text_query(pane, &query).unwrap();
    if let TextResult::Viewport { viewport, .. } = &mut result {
        viewport.screen.pop();
    }
    let message = api::Message::TextQueryResult {
        id: 41,
        pane_id: pane,
        result,
    };
    let mut malformed = Vec::new();
    ciborium::into_writer(&message, &mut malformed).unwrap();
    assert!(api::decode(&malformed).is_err());
    let TextQuery::Viewport { range, .. } = query else {
        panic!()
    };
    assert_eq!(
        core.text_query(
            pane,
            &TextQuery::Viewport {
                generation: 0,
                range
            }
        ),
        Err(PgError::InvalidArg)
    );
}

#[test]
fn viewport_is_read_only_and_other_panes_do_not_expire_its_generation() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(80, 24).unwrap();
    let other = core.split(pane, api::Axis::Vertical, 0.5).unwrap();
    core.resize(pane, 12, 2).unwrap();
    core.feed(pane, "old你é\r\nnext\r\nlast".as_bytes())
        .unwrap();
    let info = core.text_query(pane, &TextQuery::Info).unwrap().info();
    let range = api::TextRange {
        start: api::TextPoint {
            row: -1,
            col: 3,
            byte: 0,
        },
        end: api::TextPoint {
            row: -1,
            col: 5,
            byte: 3,
        },
    };
    let query = TextQuery::Viewport {
        generation: info.generation,
        range,
    };
    core.feed(other, b"unrelated output").unwrap();
    let first = core.text_query(pane, &query).unwrap();
    let second = core.text_query(pane, &query).unwrap();
    assert_eq!(first, second);
    let full = core.snapshot(pane).unwrap();
    assert_eq!(full.screen[1][0].codepoint, 'l' as u32);
    assert_eq!(full.history.len(), 1);
    assert_eq!(core.text_query(pane, &query).unwrap(), first);
}
