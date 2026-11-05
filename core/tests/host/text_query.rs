use super::*;
use pigeon_core::api::{TextQuery, TextRange, TextResult};

fn query(client: &mut Client, pane_id: u32, query: TextQuery) -> Message {
    client.send(&Message::TextQuery {
        id: 81,
        pane_id,
        query,
    });
    client.wait_for(|message| {
        matches!(
            message,
            Message::TextQueryResult { id: 81, .. } | Message::Error { id: Some(81), .. }
        )
    })
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
fn result(message: Message) -> TextResult {
    let Message::TextQueryResult { result, .. } = message else {
        panic!("{message:?}")
    };
    result
}
fn text(client: &mut Client, pane: u32, generation: u64, range: TextRange) -> String {
    let TextResult::Range { text, next, .. } =
        result(query(client, pane, TextQuery::Range { generation, range }))
    else {
        panic!()
    };
    assert_eq!(next, None);
    text
}

#[test]
fn text_query_real_pty_history_unicode_paging_limits_and_negotiation() {
    let _serial = one_at_a_time();
    // READY is the last output, including shell startup modes: pagination needs a quiet generation.
    let fixture = script("#!/bin/sh\nstty -echo\nprintf 'abc你好éZ\\r\\nhard\\r\\n'\ni=0; while [ $i -lt 300 ]; do printf 'line %s\\r\\n' \"$i\"; i=$((i+1)); done\nprintf 'READY'\nwhile IFS= read -r line; do :; done\n");
    let host = start_host_with(fixture.to_str().unwrap());
    let mut client = Client::connect(&host.socket);
    assert!(hello_with(&mut client, &["text_query"]).contains(&"text_query".into()));
    let pane = attach_first(&mut client);
    wait_for_pane_text(&mut client, pane, "READY");
    let TextResult::Search { info, hits, next } =
        result(query(&mut client, pane, search("abc你好éZ\nhard")))
    else {
        panic!()
    };
    assert_eq!(hits.len(), 1);
    assert!(hits[0].start.row < 0);
    assert!(next.is_some());
    assert_eq!(
        text(&mut client, pane, info.generation, hits[0]),
        "abc你好éZ\nhard"
    );
    // Abandon this continuation and start another query: no retained search resources.
    let mut search = search("not present");
    let mut pages = 0;
    loop {
        let TextResult::Search { info, hits, next } =
            result(query(&mut client, pane, search.clone()))
        else {
            panic!()
        };
        assert!(hits.is_empty());
        pages += 1;
        let Some(next) = next else {
            break;
        };
        if let TextQuery::Search {
            generation, start, ..
        } = &mut search
        {
            *generation = info.generation;
            *start = Some(next);
        }
        assert!(pages < 10);
    }
    assert!(pages >= 3);
    if let TextQuery::Search { max_results, .. } = &mut search {
        *max_results = 129;
    }
    assert!(
        matches!(query(&mut client, pane, search), Message::Error { code, .. } if code == "invalid_arg")
    );
    let mut invalid = hits[0];
    invalid.start.col = u16::MAX;
    invalid.end = invalid.start;
    assert!(
        matches!(query(&mut client, pane, TextQuery::Range { generation: info.generation, range: invalid }), Message::Error { code, .. } if code == "invalid_arg")
    );
    assert!(
        matches!(query(&mut client, 99999, TextQuery::Info), Message::Error { code, .. } if code == "no_such_pane")
    );
    let mut legacy = Client::connect(&host.socket);
    hello(&mut legacy);
    assert!(
        matches!(query(&mut legacy, pane, TextQuery::Info), Message::Error { code, .. } if code == "unsupported")
    );
    legacy.send(&Message::Ping { ts: 45 });
    legacy.wait_for(|m| matches!(m, Message::Pong { ts: 45 }));
}

#[test]
fn text_query_real_pty_soft_wrap_output_resize_and_alt_invalidate() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello_with(&mut client, &["text_query"]);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"stty -echo\r".to_vec(),
    });
    stable_snapshot(&mut client, pane);
    client.send(&Message::ResizePane {
        pane_id: pane,
        cols: 6,
        rows: 4,
    });
    expect_size_state(&mut client, pane, (6, 4, "latest", 1));
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: "printf '\\033[2J\\033[Habc你好éZ\\r\\nhard\\r\\nREADY'\r"
            .as_bytes()
            .to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "READY");
    let TextResult::Search { info, hits, .. } =
        result(query(&mut client, pane, search("abc你好éZ\nhard")))
    else {
        panic!()
    };
    assert_eq!(hits.len(), 1);
    assert_eq!(
        text(&mut client, pane, info.generation, hits[0]),
        "abc你好éZ\nhard"
    );
    let old = TextQuery::Range {
        generation: info.generation,
        range: hits[0],
    };
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\r\\nNEW'\r".to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "NEW");
    assert!(
        matches!(query(&mut client, pane, old), Message::Error { code, .. } if code == "stale_text")
    );
    let generation = result(query(&mut client, pane, TextQuery::Info))
        .info()
        .generation;
    client.send(&Message::ResizePane {
        pane_id: pane,
        cols: 10,
        rows: 5,
    });
    expect_size_state(&mut client, pane, (10, 5, "latest", 1));
    assert!(
        matches!(query(&mut client, pane, TextQuery::Range { generation, range: hits[0] }), Message::Error { code, .. } if code == "stale_text")
    );
    let generation = result(query(&mut client, pane, TextQuery::Info))
        .info()
        .generation;
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf '\\033[?1049hALT'\r".to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "ALT");
    assert!(
        matches!(query(&mut client, pane, TextQuery::Range { generation, range: hits[0] }), Message::Error { code, .. } if code == "stale_text")
    );
    let info = result(query(&mut client, pane, TextQuery::Info)).info();
    assert!(info.alt);
    assert_eq!(info.first_row, 0);
}

#[test]
fn text_query_real_pty_history_eviction_expires_the_old_range() {
    let _serial = one_at_a_time();
    let host = start_host();
    let mut client = Client::connect(&host.socket);
    hello_with(&mut client, &["text_query"]);
    let pane = attach_first(&mut client);
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"stty -echo; printf '\\033[2J\\033[HOLD_TOKEN\\r\\n'\r".to_vec(),
    });
    stable_snapshot(&mut client, pane);
    let TextResult::Search { info, hits, .. } =
        result(query(&mut client, pane, search("OLD_TOKEN")))
    else {
        panic!()
    };
    assert!(!hits.is_empty());
    client.send(&Message::Keys { pane_id: pane, bytes: b"i=0; while [ $i -lt 10050 ]; do printf 'line %s\\n' \"$i\"; i=$((i+1)); done; printf 'EVICTED_READY\\n'\r".to_vec() });
    wait_for_pane_text(&mut client, pane, "EVICTED_READY");
    assert!(
        matches!(query(&mut client, pane, TextQuery::Range { generation: info.generation, range: hits[0] }), Message::Error { code, .. } if code == "stale_text")
    );
    let info = result(query(&mut client, pane, TextQuery::Info)).info();
    assert_eq!(info.first_row, -10_000);
    let TextResult::Search { hits, next, .. } =
        result(query(&mut client, pane, search("OLD_TOKEN")))
    else {
        panic!()
    };
    assert!(hits.is_empty());
    assert!(next.is_some());
    // Stop without another page; the same connection remains responsive.
    client.send(&Message::Ping { ts: 82 });
    client.wait_for(|message| matches!(message, Message::Pong { ts: 82 }));
}

#[test]
fn viewport_real_pty_is_negotiated_atomic_and_does_not_change_other_views() {
    let _serial = one_at_a_time();
    let fixture = script("#!/bin/sh\nstty -echo\nprintf 'OLD_TOKEN\\r\\n'\ni=0; while [ $i -lt 40 ]; do printf 'line %s\\r\\n' \"$i\"; i=$((i+1)); done\nprintf 'READY\\r\\n'\nexec env PS1= /bin/sh -i\n");
    let host = start_host_with(fixture.to_str().unwrap());
    let mut client = Client::connect(&host.socket);
    hello_with(&mut client, &["text_query", "text_viewport"]);
    let pane = attach_first(&mut client);
    wait_for_pane_text(&mut client, pane, "READY");
    // The interactive shell exec'd after READY can still write (startup modes, an empty prompt)
    // and so advance the text generation between the search and the location. That is the
    // generation check working, not the behaviour under test, so search again until a location
    // lands on the generation it was searched in.
    let deadline = Instant::now() + Duration::from_secs(30);
    let (request, located) = loop {
        let TextResult::Search { info, hits, .. } =
            result(query(&mut client, pane, search("OLD_TOKEN")))
        else {
            panic!()
        };
        let request = TextQuery::Viewport {
            generation: info.generation,
            range: hits[0],
        };
        match query(&mut client, pane, request.clone()) {
            Message::Error { code, .. } if code == "stale_text" && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            message => break (request, result(message)),
        }
    };
    let picture = pigeon_core::api::text_viewport_snapshot(&located).unwrap();
    assert!(picture.history.is_empty());
    assert_eq!(picture.screen[0][0].codepoint, 'O' as u32);
    let mut legacy = Client::connect(&host.socket);
    hello_with(&mut legacy, &["text_query"]);
    assert!(
        matches!(query(&mut legacy, pane, request.clone()), Message::Error { id: Some(81), code, .. } if code == "unsupported")
    );
    // A location reads a picture, never changes another connection's scroll position.
    legacy.send(&Message::Scroll {
        pane_id: pane,
        offset: 0,
    });
    legacy.wait_for(|m| {
        matches!(
            m,
            Message::Snapshot {
                offset: Some(0),
                ..
            }
        )
    });
    client.send(&Message::Keys {
        pane_id: pane,
        bytes: b"printf 'CHANGED\\n'\n".to_vec(),
    });
    wait_for_pane_text(&mut client, pane, "CHANGED");
    assert!(
        matches!(query(&mut client, pane, request), Message::Error { id: Some(81), code, .. } if code == "stale_text")
    );
}
