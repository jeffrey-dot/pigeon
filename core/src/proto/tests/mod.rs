use super::*;

mod samples;
use samples::samples;

#[test]
fn every_message_survives_a_round_trip() {
    for message in samples() {
        let payload = encode(&message).expect("encode");
        let decoded = decode(&payload).expect("decode");
        assert_eq!(decoded, message, "round-trip failed for {}", message.name());
    }
}

#[test]
fn appearance_without_colours_is_how_a_legacy_client_speaks() {
    // A client that never learned about `appearance` simply does not send it; one that
    // sends an empty message must decode to "no colours", not to a panic or a default.
    let decoded = decode(
        &encode(&Message::Appearance {
            fg: None,
            bg: None,
            ansi: Vec::new(),
        })
        .expect("encode"),
    )
    .expect("decode");
    assert_eq!(
        decoded,
        Message::Appearance {
            fg: None,
            bg: None,
            ansi: Vec::new()
        }
    );
    match decoded {
        Message::Appearance { fg, bg, ansi } => {
            assert!(fg.is_none() && bg.is_none() && ansi.is_empty());
        }
        other => panic!("expected appearance, got {other:?}"),
    }
}

#[test]
fn appearance_carries_a_wrong_length_palette_through_untouched() {
    // Validation belongs to the host: the codec must not silently pad or truncate.
    let ansi = vec![0x00_00_00; 12];
    let decoded = decode(
        &encode(&Message::Appearance {
            fg: Some(0x00ff_ffff),
            bg: Some(0x0005_0505),
            ansi: ansi.clone(),
        })
        .expect("encode"),
    )
    .expect("decode");
    match decoded {
        Message::Appearance { ansi: got, .. } => assert_eq!(got, ansi),
        other => panic!("expected appearance, got {other:?}"),
    }
}

#[test]
fn cell_without_link_serializes_to_four_elements() {
    let cell = Cell {
        codepoint: b'A' as u32,
        attrs: 0,
        fg: 0,
        bg: 0,
        link: None,
    };
    let value = ciborium::value::Value::serialized(&cell).expect("value");
    let ciborium::value::Value::Array(items) = value else {
        panic!("cell must be an array");
    };
    assert_eq!(items.len(), 4);
}

#[test]
fn cell_with_link_serializes_to_five_elements() {
    let cell = Cell {
        codepoint: b'A' as u32,
        attrs: 0,
        fg: 0,
        bg: 0,
        link: Some(3),
    };
    let value = ciborium::value::Value::serialized(&cell).expect("value");
    let ciborium::value::Value::Array(items) = value else {
        panic!("cell must be an array");
    };
    assert_eq!(items.len(), 5);
}

#[test]
fn frame_carries_a_little_endian_length() {
    let frame = encode_frame(&Message::Ping { ts: 7 }).expect("frame");
    let declared = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    assert_eq!(declared, frame.len() - 4);
    let (message, consumed) = decode_frame(&frame).expect("decode").expect("complete");
    assert_eq!(message, Message::Ping { ts: 7 });
    assert_eq!(consumed, frame.len());
}

#[test]
fn an_unknown_frame_still_reports_its_length() {
    let payload = encode(&Message::Ping { ts: 1 }).expect("encode");
    let unknown = {
        let value = Value::Map(vec![(
            Value::Text("t".into()),
            Value::Text("from_the_future".into()),
        )]);
        let mut body = Vec::new();
        ciborium::into_writer(&value, &mut body).expect("encode");
        body
    };
    let mut buf = (unknown.len() as u32).to_le_bytes().to_vec();
    buf.extend_from_slice(&unknown);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    let (first, used) = split_frame(&buf).expect("ok").expect("complete");
    assert_eq!(decode(first), Err(PgError::Unsupported));
    assert_eq!(used, 4 + unknown.len());
    let (next, _) = decode_frame(&buf[used..]).expect("ok").expect("complete");
    assert_eq!(next, Message::Ping { ts: 1 });
    // A malformed header is still an error, not a skip.
    let mut huge = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
    huge.push(0);
    assert_eq!(split_frame(&huge), Err(PgError::Overflow));
}

#[test]
fn half_a_frame_asks_for_more() {
    let frame = encode_frame(&Message::Ping { ts: 7 }).expect("frame");
    for cut in 0..frame.len() {
        assert!(
            decode_frame(&frame[..cut]).expect("no error").is_none(),
            "prefix of {cut} bytes must be incomplete"
        );
    }
}

#[test]
fn declared_length_over_the_cap_is_rejected() {
    let mut frame = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
    frame.push(0);
    assert_eq!(decode_frame(&frame), Err(PgError::Overflow));
}

#[test]
fn encoding_a_payload_over_the_cap_is_rejected() {
    let big = vec![
        Cell {
            codepoint: 0,
            attrs: 0,
            fg: 0,
            bg: 0,
            link: None
        };
        MAX_FRAME + 1
    ];
    let message = Message::Snapshot {
    text: Vec::new(),
        pane_id: 1,
        seq: 1,
        cols: 1,
        rows: 1,
        alt: false,
        cursor: Cursor {
            x: 0,
            y: 0,
            visible: true,
            shape: 0,
        },
        scroll: Scroll { top: 0, bottom: 0 },
        screen: vec![big],
        history: None,
        links: None,
        history_len: None,
        offset: None,
        modes: None,
    };
    assert_eq!(encode_frame(&message), Err(PgError::Overflow));
}

#[test]
fn unknown_message_type_is_unsupported_not_fatal() {
    let payload = ciborium::value::Value::Map(vec![
        ("t".into(), "definitely_not_a_message".into()),
        ("pane_id".into(), 1u32.into()),
    ]);
    let mut bytes = Vec::new();
    ciborium::into_writer(&payload, &mut bytes).expect("encode");
    assert_eq!(decode(&bytes), Err(PgError::Unsupported));
}

#[test]
fn wrong_field_type_is_invalid() {
    let payload = ciborium::value::Value::Map(vec![
        ("t".into(), "hello".into()),
        ("proto".into(), "one".into()),
        ("client".into(), "x".into()),
        ("features".into(), ciborium::value::Value::Array(vec![])),
    ]);
    let mut bytes = Vec::new();
    ciborium::into_writer(&payload, &mut bytes).expect("encode");
    assert_eq!(decode(&bytes), Err(PgError::InvalidArg));
}

#[test]
fn payload_without_a_type_tag_is_invalid() {
    let mut bytes = Vec::new();
    ciborium::into_writer(&ciborium::value::Value::Map(vec![]), &mut bytes).expect("encode");
    assert_eq!(decode(&bytes), Err(PgError::InvalidArg));
}

#[test]
fn layout_tree_decodes_with_focus_and_reports_leaf_order() {
    let message = Message::Layout {
        session_id: 7,
        tree: Node::Split {
            split: SplitNode {
                id: 1,
                axis: SplitAxis::Vertical,
                ratio: 0.4,
                a: Box::new(Node::Leaf { leaf: 10 }),
                b: Box::new(Node::Split {
                    split: SplitNode {
                        id: 2,
                        axis: SplitAxis::Horizontal,
                        ratio: 0.5,
                        a: Box::new(Node::Leaf { leaf: 11 }),
                        b: Box::new(Node::Leaf { leaf: 12 }),
                    },
                }),
            },
        },
        zoom: None,
        focus: Some(11),
    };
    let payload = encode(&message).expect("encode");
    let decoded = decode(&payload).expect("decode");
    assert_eq!(decoded, message);
    let Message::Layout { tree, focus, .. } = decoded else {
        panic!("layout");
    };
    let mut leaves = Vec::new();
    tree.leaves(&mut leaves);
    assert_eq!(leaves, vec![10, 11, 12], "tree order");
    assert_eq!(focus, Some(11));
}

#[test]
fn malformed_split_axis_is_rejected() {
    let payload = ciborium::value::Value::Map(vec![
        ("t".into(), "split_pane".into()),
        ("id".into(), 1u32.into()),
        ("pane_id".into(), 1u32.into()),
        ("axis".into(), "x".into()),
        ("ratio".into(), 0.5f32.into()),
    ]);
    let mut bytes = Vec::new();
    ciborium::into_writer(&payload, &mut bytes).expect("encode");
    assert_eq!(decode(&bytes), Err(PgError::InvalidArg));
}

#[test]
fn malformed_layout_ratio_is_rejected() {
    let tree = ciborium::value::Value::Map(vec![(
        "split".into(),
        ciborium::value::Value::Map(vec![
            ("id".into(), 1u32.into()),
            ("axis".into(), "v".into()),
            ("ratio".into(), "half".into()),
            (
                "a".into(),
                ciborium::value::Value::Map(vec![("leaf".into(), 1u32.into())]),
            ),
            (
                "b".into(),
                ciborium::value::Value::Map(vec![("leaf".into(), 2u32.into())]),
            ),
        ]),
    )]);
    let payload = ciborium::value::Value::Map(vec![
        ("t".into(), "layout".into()),
        ("session_id".into(), 1u32.into()),
        ("tree".into(), tree),
    ]);
    let mut bytes = Vec::new();
    ciborium::into_writer(&payload, &mut bytes).expect("encode");
    assert_eq!(decode(&bytes), Err(PgError::InvalidArg));
}

#[test]
fn zoom_messages_round_trip() {
    for message in [
        Message::Scroll {
            pane_id: 1,
            offset: -5,
        },
        Message::ZoomPane { id: 7, pane_id: 1 },
        Message::Unzoom { id: 8, pane_id: 1 },
    ] {
        let bytes = encode(&message).expect("encode");
        assert_eq!(decode(&bytes).expect("decode"), message);
    }
}
