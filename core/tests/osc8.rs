#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use pigeon_core::api::{Config, Core};

#[test]
fn osc8_cells_own_links_and_overwrite_clears_them() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(12, 3).unwrap();
    core.feed(
        pane,
        b"\x1b]8;id=test;https://example.com\x1b\\Hi\x1b]8;;\x07!",
    )
    .unwrap();
    let old = core.snapshot(pane).unwrap();
    assert_eq!(
        old.screen[0][0].hyperlink.as_deref(),
        Some("https://example.com")
    );
    assert_eq!(old.screen[0][1].link, old.screen[0][0].link);
    assert!(old.screen[0][2].hyperlink.is_none());
    core.feed(pane, b"\rX\x1b[K").unwrap();
    assert!(core.snapshot(pane).unwrap().screen[0]
        .iter()
        .all(|cell| cell.hyperlink.is_none()));
    drop(core);
    assert_eq!(
        old.screen[0][0].hyperlink.as_deref(),
        Some("https://example.com")
    );
}

#[test]
fn raw_osc8_controls_invalid_utf8_and_oversize_are_rejected() {
    for uri in [
        b"https://ex\x01ample.com".to_vec(),
        b"https://example.com/\xff".to_vec(),
        b"https://example.com/\xc2\x85".to_vec(),
        b"https://example.com/;;;;;;;;;;;;;;".to_vec(),
        vec![b'x'; 3000],
    ] {
        let mut core = Core::new(Config::default());
        let pane = core.new_pane(12, 3).unwrap();
        core.feed(pane, b"\x1b]8;;https://good.test\x07").unwrap();
        let mut bytes = b"\x1b]8;;".to_vec();
        bytes.extend(uri);
        bytes.extend_from_slice(b"\x1b\\X");
        for chunk in bytes.chunks(3) {
            core.feed(pane, chunk).unwrap();
        }
        let snapshot = core.snapshot(pane).unwrap();
        assert_eq!(snapshot.screen[0][0].codepoint, u32::from('X'));
        assert!(snapshot.screen[0][0].hyperlink.is_none());
    }
}

#[test]
fn links_survive_wide_cells_wrap_resize_alt_and_history() {
    let mut core = Core::new(Config {
        scrollback_lines: 10,
        ..Config::default()
    });
    let pane = core.new_pane(4, 3).unwrap();
    core.feed(
        pane,
        "\x1b]8;;https://example.com/a;b\x07中文abcdef\x1b]8;;\x07\r\nend\r\n".as_bytes(),
    )
    .unwrap();
    let before = core.snapshot(pane).unwrap();
    let linked = |snap: &pigeon_core::api::Snapshot| {
        snap.history
            .iter()
            .chain(&snap.screen)
            .flatten()
            .filter_map(|cell| cell.hyperlink.clone())
            .collect::<Vec<_>>()
    };
    assert!(!before.history.is_empty());
    assert!(linked(&before)
        .iter()
        .all(|uri| uri.as_ref() == "https://example.com/a;b"));
    assert_eq!(before.history[0][1].width, 0);
    assert!(before.history[0][1].hyperlink.is_some());
    core.feed(
        pane,
        b"\x1b[?1049h\x1b[H\x1b]8;;https://alt.test\x07ALT\x1b]8;;\x07",
    )
    .unwrap();
    assert_eq!(
        core.snapshot(pane).unwrap().screen[0][0]
            .hyperlink
            .as_deref(),
        Some("https://alt.test")
    );
    core.feed(pane, b"\x1b[?1049l").unwrap();
    assert!(before.same_screen(&core.snapshot(pane).unwrap()));
    core.resize(pane, 8, 4).unwrap();
    let resized = core.snapshot(pane).unwrap();
    assert_eq!(linked(&before), linked(&resized));
    core.feed(pane, b"\x1bc\x1b]8;;https://reset.test\x07R")
        .unwrap();
    let reset = core.snapshot(pane).unwrap();
    assert_ne!(reset.screen[0][0].link, before.history[0][0].link);
}

#[test]
fn cancelled_osc8_prefix_closes_the_previous_link() {
    for cancel in [0x18, 0x1a] {
        for chunk_size in [1, 2, 64] {
            let mut core = Core::new(Config::default());
            let pane = core.new_pane(12, 3).unwrap();
            core.feed(pane, b"\x1b]8;;https://old.test\x07").unwrap();
            let bytes = [0x1b, b']', b'8', cancel, b'X'];
            for chunk in bytes.chunks(chunk_size) {
                core.feed(pane, chunk).unwrap();
            }
            let snapshot = core.snapshot(pane).unwrap();
            assert_eq!(snapshot.screen[0][0].codepoint, u32::from('X'));
            assert!(snapshot.screen[0][0].hyperlink.is_none());
        }
    }
}

#[test]
fn nested_osc_and_cancellation_cannot_bypass_raw_validation() {
    for bytes in [
        b"\x1b]8;;https://first.test\x1b]8;id=pigeon-1;https://ex\x01ample.test\x07X".as_slice(),
        b"\x1b]8;;https://first.test\x18X",
        b"\x1bPqignored\x1b]8;id=pigeon-1;https://ex\x01ample.test\x07X",
        b"\x1b\x00]8;id=pigeon-1;https://ex\x01ample.test\x07X",
        b"\x1b]\x008;id=pigeon-1;https://example.test\x07X",
        b"\x1b]8\x00;id=pigeon-1;https://example.test\x07X",
        b"\x1b]8;malformed\x07X",
    ] {
        let mut core = Core::new(Config::default());
        let pane = core.new_pane(20, 3).unwrap();
        for byte in bytes {
            core.feed(pane, &[*byte]).unwrap();
        }
        let snap = core.snapshot(pane).unwrap();
        assert!(snap
            .screen
            .iter()
            .flatten()
            .all(|cell| cell.hyperlink.is_none()));
        assert!(snap
            .screen
            .iter()
            .flatten()
            .any(|cell| cell.codepoint == u32::from('X')));
    }
}

#[cfg(feature = "internals")]
mod wire {
    use pigeon_core::{api::*, session::PaneState};

    fn roundtrip(message: &Message) -> Message {
        let bytes = encode_frame(message).unwrap();
        decode_frame(&bytes).unwrap().unwrap().0
    }

    #[test]
    fn snapshot_delta_scroll_resume_keep_owned_targets() {
        let mut pane = PaneState::new(1, 10, 3, 20);
        pane.feed(b"\x1b]8;id=same;https://old.test\x07old\x1b]8;;\x07\r\n");
        let first = pane.delta_message().unwrap();
        let old = snapshot_of(&roundtrip(&pane.snapshot_message(false)))
            .unwrap()
            .1;
        let seq = delta_of(&first).unwrap().seq;
        pane.feed(b"\x1b]8;id=same;https://new.test\x07new\x1b]8;;\x07");
        let newer = pane.delta_message().unwrap();
        let delta = delta_of(&roundtrip(&newer)).unwrap();
        assert_eq!(
            old.screen[0][0].hyperlink.as_deref(),
            Some("https://old.test")
        );
        assert_eq!(
            delta.rows.iter().find(|(y, _)| *y == 1).unwrap().1[0]
                .hyperlink
                .as_deref(),
            Some("https://new.test")
        );
        assert_ne!(
            old.screen[0][0].link,
            delta.rows.iter().find(|(y, _)| *y == 1).unwrap().1[0].link
        );
        assert_eq!(pane.deltas_since(seq).unwrap(), vec![newer]);
        pane.feed(b"\r\nthird\r\nfourth\r\n");
        let _ = pane.delta_message();
        let history = snapshot_of(&roundtrip(&pane.snapshot_message(true)))
            .unwrap()
            .1;
        assert_eq!(
            history.history[0][0].hyperlink.as_deref(),
            Some("https://old.test")
        );
        let (message, offset) = pane.scroll_message(99);
        assert!(offset > 0);
        let scroll = snapshot_of(&roundtrip(&message)).unwrap().1;
        assert_eq!(
            scroll.screen[0][0].hyperlink.as_deref(),
            Some("https://old.test")
        );
        let mut recovered = PaneState::from_snapshot(history.clone());
        assert_eq!(
            snapshot_of(&roundtrip(&recovered.snapshot_message(true)))
                .unwrap()
                .1,
            history
        );
    }

    #[test]
    fn unknown_and_legacy_ids_are_inert_and_bad_tables_are_rejected() {
        let mut pane = PaneState::new(1, 4, 2, 10);
        pane.feed(b"\x1b]8;;https://good.test\x07ok\x1b]8;;\x07");
        let original = pane.snapshot_message(false);
        let mut legacy = original.clone();
        if let Message::Snapshot { links, .. } = &mut legacy {
            *links = None;
        }
        let picture = snapshot_of(&roundtrip(&legacy)).unwrap().1;
        assert!(picture
            .screen
            .iter()
            .flatten()
            .all(|cell| cell.link == 0 && cell.hyperlink.is_none()));
        for links in [
            vec![Link {
                id: 0,
                uri: "https://test".into(),
            }],
            vec![Link {
                id: 1,
                uri: "https://ex\x01ample".into(),
            }],
            vec![Link {
                id: 1,
                uri: "x".repeat(2049),
            }],
            vec![
                Link {
                    id: 1,
                    uri: "https://test".into()
                };
                2
            ],
        ] {
            let mut malformed = original.clone();
            if let Message::Snapshot { links: table, .. } = &mut malformed {
                *table = Some(links);
            }
            assert!(snapshot_of(&malformed).is_err());
            assert!(encode(&malformed).is_err());
            let mut raw = Vec::new();
            ciborium::into_writer(&malformed, &mut raw).unwrap();
            assert!(decode(&raw).is_err());
        }
        let mut missing = original;
        if let Message::Snapshot { links, .. } = &mut missing {
            links.as_mut().unwrap()[0].id = 999;
        }
        assert!(snapshot_of(&roundtrip(&missing)).unwrap().1.screen[0][0]
            .hyperlink
            .is_none());
    }

    #[test]
    fn link_count_and_bytes_are_bounded_without_losing_text() {
        let mut pane = PaneState::new(1, 1100, 1, 0);
        for n in 0..1100 {
            pane.feed(format!("\x1b]8;;https://example.com/{n}\x07x").as_bytes());
        }
        let message = pane.snapshot_message(false);
        let picture = snapshot_of(&roundtrip(&message)).unwrap().1;
        assert!(picture.screen[0]
            .iter()
            .all(|cell| cell.codepoint == u32::from('x')));
        assert_eq!(
            picture.screen[0]
                .iter()
                .filter(|cell| cell.hyperlink.is_some())
                .count(),
            1024
        );
        let mut pane = PaneState::new(2, 200, 1, 0);
        for n in 0..200 {
            pane.feed(
                format!("\x1b]8;;https://example.com/{n}/{}\x07x", "a".repeat(2000)).as_bytes(),
            );
        }
        let message = pane.snapshot_message(false);
        let Message::Snapshot {
            links: Some(links), ..
        } = &message
        else {
            panic!("links");
        };
        assert!(links.iter().map(|link| link.uri.len()).sum::<usize>() <= 256 * 1024);
        assert_eq!(
            snapshot_of(&roundtrip(&message)).unwrap().1.screen[0].len(),
            200
        );
    }
}

#[test]
fn oversized_unterminated_osc_recovers_without_corrupting_other_sequences() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(20, 2).unwrap();
    core.feed(pane, b"\x1b]8;;").unwrap();
    for _ in 0..1024 {
        core.feed(pane, &[b'x'; 1024]).unwrap();
    }
    core.feed(
        pane,
        b"\x07safe\x1b[31mR\x1b[0m\x1b]0;title\x07\x1b]8;;https://good.test\x07G",
    )
    .unwrap();
    let snap = core.snapshot(pane).unwrap();
    assert_eq!(snap.screen[0][0].codepoint, u32::from('s'));
    assert!(snap.screen[0][0].hyperlink.is_none());
    assert_eq!(snap.screen[0][4].fg, 0xcd0000);
    assert_eq!(
        snap.screen[0][5].hyperlink.as_deref(),
        Some("https://good.test")
    );
}
