//! Wire protocol: `u32` little-endian length prefix + one CBOR map.
//!
//! The single implementation of framing and encoding in the repository. The
//! client reaches it through the C ABI, never by re-implementing CBOR.
//!
//! Contract: `docs/protocol.md` §1–§4. M0 implements the subset listed in
//! [`Message`]; anything else decodes to [`PgError::Unsupported`] so the host can
//! answer `unsupported` without dropping the connection.

use std::collections::BTreeMap;
use std::fmt;

use ciborium::value::Value;
use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{PgError, PgResult};

/// A frame may not exceed 8 MiB (protocol §1).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// Exact wire version: major 1, minor 0.
pub const PROTO_VERSION: u16 = 0x0100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub shape: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scroll {
    pub top: u16,
    pub bottom: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub id: u32,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: u32,
    pub title: String,
    pub cwd: String,
    pub panes: Vec<u32>,
    /// Pane the session's layout has focused (M3, optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_pane: Option<u32>,
    /// Title of that pane (OSC 0/2), so the sidebar can name the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub y: u16,
    pub cells: Vec<Cell>,
}

/// `[codepoint, attrs, fg, bg, link?]`. The fifth element is omitted when the
/// cell has no hyperlink (protocol §3.3); `null` is never emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub codepoint: u32,
    pub attrs: u8,
    pub fg: u32,
    pub bg: u32,
    pub link: Option<u32>,
}

impl Serialize for Cell {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = if self.link.is_some() { 5 } else { 4 };
        let mut tuple = serializer.serialize_tuple(len)?;
        tuple.serialize_element(&self.codepoint)?;
        tuple.serialize_element(&self.attrs)?;
        tuple.serialize_element(&self.fg)?;
        tuple.serialize_element(&self.bg)?;
        if let Some(link) = self.link {
            tuple.serialize_element(&link)?;
        }
        tuple.end()
    }
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CellVisitor;

        impl<'de> Visitor<'de> for CellVisitor {
            type Value = Cell;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array of 4 or 5 unsigned integers")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Cell, A::Error> {
                let codepoint = seq
                    .next_element::<u32>()?
                    .ok_or_else(|| de::Error::custom("codepoint"))?;
                let attrs = seq
                    .next_element::<u8>()?
                    .ok_or_else(|| de::Error::custom("attrs"))?;
                let fg = seq
                    .next_element::<u32>()?
                    .ok_or_else(|| de::Error::custom("fg"))?;
                let bg = seq
                    .next_element::<u32>()?
                    .ok_or_else(|| de::Error::custom("bg"))?;
                let link = seq.next_element::<u32>()?;
                if seq.next_element::<Value>()?.is_some() {
                    return Err(de::Error::custom("cell has more than 5 elements"));
                }
                Ok(Cell {
                    codepoint,
                    attrs,
                    fg,
                    bg,
                    link,
                })
            }
        }

        deserializer.deserialize_tuple(5, CellVisitor)
    }
}

/// Which way a split divides: `v` = side by side, `h` = stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    #[serde(rename = "v")]
    Vertical,
    #[serde(rename = "h")]
    Horizontal,
}

/// One node of the layout tree, exactly as `docs/protocol.md` §3.2 spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Leaf { leaf: u32 },
    Split { split: SplitNode },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitNode {
    pub id: u32,
    pub axis: SplitAxis,
    pub ratio: f32,
    pub a: Box<Node>,
    pub b: Box<Node>,
}

impl Node {
    /// Leaves in tree order, left-to-right / top-to-bottom.
    pub fn leaves(&self, out: &mut Vec<u32>) {
        match self {
            Self::Leaf { leaf } => out.push(*leaf),
            Self::Split { split } => {
                split.a.leaves(out);
                split.b.leaves(out);
            }
        }
    }
}

/// Every message the M1 host and client speak. `t` carries the snake-case name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Message {
    Hello {
        proto: u16,
        client: String,
        features: Vec<String>,
    },
    HelloAck {
        proto: u16,
        server: String,
        features: Vec<String>,
        host_id: String,
        client_id: u32,
    },
    HelloError {
        reason: String,
        detail: String,
    },
    Sessions {
        /// Empty in a client request (`pg_msg_new(PG_MSG_SESSIONS)` + encode); the host always
        /// answers with the full list.
        #[serde(default)]
        items: Vec<SessionInfo>,
    },
    NewSession {
        id: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        env: Option<BTreeMap<String, String>>,
    },
    Created {
        id: u32,
        session_id: u32,
        pane_id: u32,
    },
    ClosePane {
        id: u32,
        pane_id: u32,
    },
    SplitPane {
        id: u32,
        pane_id: u32,
        axis: SplitAxis,
        ratio: f32,
    },
    ResizeSplit {
        id: u32,
        split_id: u32,
        ratio: f32,
    },
    Focus {
        pane_id: u32,
    },
    Layout {
        session_id: u32,
        tree: Node,
        #[serde(skip_serializing_if = "Option::is_none")]
        zoom: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        focus: Option<u32>,
    },
    Attach {
        id: u32,
        pane_id: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        size_policy: Option<String>,
    },
    Snapshot {
        pane_id: u32,
        seq: u32,
        cols: u16,
        rows: u16,
        alt: bool,
        cursor: Cursor,
        scroll: Scroll,
        screen: Vec<Vec<Cell>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        history: Option<Vec<Vec<Cell>>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        links: Option<Vec<Link>>,
    },
    Delta {
        pane_id: u32,
        seq: u32,
        rows: Vec<Row>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
        #[serde(skip_serializing_if = "Option::is_none")]
        scroll: Option<Scroll>,
        #[serde(skip_serializing_if = "Option::is_none")]
        links: Option<Vec<Link>>,
    },
    /// The client's current theme, so the host can answer `OSC 4/10/11` colour queries for
    /// the programs inside the panes. All fields are optional: a client that never sends this
    /// (or sends it without colours) leaves the host's behaviour exactly as it was.
    Appearance {
        #[serde(skip_serializing_if = "Option::is_none")]
        fg: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bg: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ansi: Vec<u32>,
    },
    Keys {
        pane_id: u32,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    ResizePane {
        pane_id: u32,
        cols: u16,
        rows: u16,
    },
    /// Look back into the scrollback (protocol §3.3): a negative `offset` moves that many
    /// lines into history, `0` returns to the live screen.
    Scroll {
        pane_id: u32,
        offset: i32,
    },
    /// Zoom one pane to the whole window (protocol §3.2).
    ZoomPane {
        id: u32,
        pane_id: u32,
    },
    /// Give every pane its place in the tree back.
    Unzoom {
        id: u32,
        pane_id: u32,
    },
    /// Broadcast after every size change: who won the size policy and how many
    /// clients are watching (protocol §6).
    SizeState {
        pane_id: u32,
        cols: u16,
        rows: u16,
        source: String,
        clients: u8,
    },
    PaneExited {
        pane_id: u32,
        code: i32,
    },
    Status {
        pane_id: u32,
        state: String,
        seq: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
    },
    AckStatus {
        pane_id: u32,
        seq: u32,
    },
    Ping {
        ts: u64,
    },
    Pong {
        ts: u64,
    },
    Error {
        code: String,
        detail: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
    },
    Unsupported {
        t_of: String,
    },
}

impl Message {
    /// The value carried in `t`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::HelloAck { .. } => "hello_ack",
            Self::HelloError { .. } => "hello_error",
            Self::Sessions { .. } => "sessions",
            Self::NewSession { .. } => "new_session",
            Self::Created { .. } => "created",
            Self::ClosePane { .. } => "close_pane",
            Self::SplitPane { .. } => "split_pane",
            Self::ResizeSplit { .. } => "resize_split",
            Self::Focus { .. } => "focus",
            Self::Layout { .. } => "layout",
            Self::Attach { .. } => "attach",
            Self::Snapshot { .. } => "snapshot",
            Self::Delta { .. } => "delta",
            Self::Appearance { .. } => "appearance",
            Self::Keys { .. } => "keys",
            Self::ResizePane { .. } => "resize_pane",
            Self::Scroll { .. } => "scroll",
            Self::ZoomPane { .. } => "zoom_pane",
            Self::Unzoom { .. } => "unzoom",
            Self::SizeState { .. } => "size_state",
            Self::PaneExited { .. } => "pane_exited",
            Self::Status { .. } => "status",
            Self::AckStatus { .. } => "ack_status",
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
            Self::Error { .. } => "error",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

/// Serializes one message into its CBOR payload (no length prefix).
pub fn encode(message: &Message) -> PgResult<Vec<u8>> {
    let mut payload = Vec::new();
    ciborium::into_writer(message, &mut payload).map_err(|_| PgError::InvalidArg)?;
    Ok(payload)
}

/// Parses one CBOR payload. Unknown `t` is [`PgError::Unsupported`]; anything
/// else that does not match the schema is [`PgError::InvalidArg`].
pub fn decode(payload: &[u8]) -> PgResult<Message> {
    let value: Value = ciborium::from_reader(payload).map_err(|_| PgError::InvalidArg)?;

    let tag = match &value {
        Value::Map(entries) => entries.iter().find_map(|(key, value)| match (key, value) {
            (Value::Text(name), Value::Text(variant)) if name == "t" => Some(variant.clone()),
            _ => None,
        }),
        _ => None,
    };
    let Some(tag) = tag else {
        return Err(PgError::InvalidArg);
    };
    if !is_known(&tag) {
        return Err(PgError::Unsupported);
    }
    value
        .deserialized::<Message>()
        .map_err(|_| PgError::InvalidArg)
}

/// Prefixes a payload with its little-endian length.
pub fn encode_frame(message: &Message) -> PgResult<Vec<u8>> {
    let payload = encode(message)?;
    if payload.len() > MAX_FRAME {
        return Err(PgError::Overflow);
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Reads one frame from the front of `buf`.
///
/// `Ok(None)` means "need more bytes" — the caller keeps accumulating. On
/// success the returned length says how many bytes were consumed.
pub fn decode_frame(buf: &[u8]) -> PgResult<Option<(Message, usize)>> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let length = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if length > MAX_FRAME {
        return Err(PgError::Overflow);
    }
    if buf.len() < 4 + length {
        return Ok(None);
    }
    let message = decode(&buf[4..4 + length])?;
    Ok(Some((message, 4 + length)))
}

fn is_known(tag: &str) -> bool {
    matches!(
        tag,
        "hello"
            | "hello_ack"
            | "hello_error"
            | "sessions"
            | "new_session"
            | "created"
            | "close_pane"
            | "split_pane"
            | "resize_split"
            | "focus"
            | "layout"
            | "attach"
            | "snapshot"
            | "delta"
            | "appearance"
            | "keys"
            | "resize_pane"
            | "scroll"
            | "zoom_pane"
            | "unzoom"
            | "size_state"
            | "pane_exited"
            | "status"
            | "ack_status"
            | "ping"
            | "pong"
            | "error"
            | "unsupported"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cell() -> Cell {
        Cell {
            codepoint: 'é' as u32,
            attrs: 0b101,
            fg: 0x0100_0002,
            bg: 0x0012_3456,
            link: None,
        }
    }

    fn samples() -> Vec<Message> {
        vec![
            Message::Hello {
                proto: PROTO_VERSION,
                client: "pigeon-mac".into(),
                features: vec!["delta".into()],
            },
            Message::HelloAck {
                proto: PROTO_VERSION,
                server: "pigeon-host".into(),
                features: vec!["delta".into(), "scrollback".into()],
                host_id: "local".into(),
                client_id: 7,
            },
            Message::HelloError {
                reason: "version".into(),
                detail: "need 1.x".into(),
            },
            Message::Sessions {
                items: vec![
                    SessionInfo {
                        session_id: 1,
                        title: "pigeon".into(),
                        cwd: "/tmp".into(),
                        panes: vec![1, 2],
                        active_pane: None,
                        active_title: None,
                    },
                    SessionInfo {
                        session_id: 2,
                        title: "web".into(),
                        cwd: "/tmp/web".into(),
                        panes: vec![3],
                        active_pane: Some(3),
                        active_title: Some("vim".into()),
                    },
                ],
            },
            Message::NewSession {
                id: 1,
                cwd: None,
                shell: None,
                env: None,
            },
            Message::NewSession {
                id: 2,
                cwd: Some("/tmp".into()),
                shell: Some("/bin/zsh".into()),
                env: Some(BTreeMap::from([(
                    "PI_PROGRAM_STATUS".to_owned(),
                    "1".to_owned(),
                )])),
            },
            Message::Created {
                id: 3,
                session_id: 1,
                pane_id: 1,
            },
            Message::ClosePane { id: 4, pane_id: 1 },
            Message::SplitPane {
                id: 5,
                pane_id: 1,
                axis: SplitAxis::Vertical,
                ratio: 0.5,
            },
            Message::ResizeSplit {
                id: 6,
                split_id: 2,
                ratio: 0.35,
            },
            Message::Focus { pane_id: 3 },
            Message::Layout {
                session_id: 1,
                tree: Node::Split {
                    split: SplitNode {
                        id: 2,
                        axis: SplitAxis::Horizontal,
                        ratio: 0.5,
                        a: Box::new(Node::Leaf { leaf: 1 }),
                        b: Box::new(Node::Split {
                            split: SplitNode {
                                id: 3,
                                axis: SplitAxis::Vertical,
                                ratio: 0.25,
                                a: Box::new(Node::Leaf { leaf: 2 }),
                                b: Box::new(Node::Leaf { leaf: 3 }),
                            },
                        }),
                    },
                },
                zoom: None,
                focus: Some(3),
            },
            Message::Attach {
                id: 5,
                pane_id: 1,
                size_policy: None,
            },
            Message::Attach {
                id: 6,
                pane_id: 1,
                size_policy: Some("latest".into()),
            },
            Message::Snapshot {
                pane_id: 1,
                seq: 1,
                cols: 2,
                rows: 1,
                alt: false,
                cursor: Cursor {
                    x: 1,
                    y: 0,
                    visible: true,
                    shape: 0,
                },
                scroll: Scroll { top: 0, bottom: 0 },
                screen: vec![vec![
                    sample_cell(),
                    Cell {
                        link: Some(9),
                        ..sample_cell()
                    },
                ]],
                history: Some(vec![vec![sample_cell(), sample_cell()]]),
                links: Some(vec![Link {
                    id: 9,
                    uri: "https://example.com".into(),
                }]),
            },
            Message::Delta {
                pane_id: 1,
                seq: 2,
                rows: vec![Row {
                    y: 0,
                    cells: vec![sample_cell()],
                }],
                cursor: Some(Cursor {
                    x: 0,
                    y: 0,
                    visible: false,
                    shape: 2,
                }),
                scroll: None,
                links: None,
            },
            Message::Appearance {
                fg: Some(0x0011_1111),
                bg: Some(0x00ff_ffff),
                ansi: vec![
                    0x212121, 0xC30771, 0x10A778, 0xA89C14, 0x008EC4, 0x523C79, 0x20A5BA, 0xE0E0E0,
                    0x212121, 0xFB007A, 0x5FD7AF, 0xF3E430, 0x20BBFC, 0x6855DE, 0x4FB8CC, 0xF1F1F1,
                ],
            },
            Message::Keys {
                pane_id: 1,
                bytes: vec![0x1b, b'[', b'A'],
            },
            Message::ResizePane {
                pane_id: 1,
                cols: 120,
                rows: 40,
            },
            Message::SizeState {
                pane_id: 1,
                cols: 120,
                rows: 40,
                source: "latest".into(),
                clients: 2,
            },
            Message::Scroll {
                pane_id: 1,
                offset: -5,
            },
            Message::ZoomPane { id: 7, pane_id: 1 },
            Message::Unzoom { id: 8, pane_id: 1 },
            Message::PaneExited {
                pane_id: 1,
                code: -1,
            },
            Message::Ping { ts: 1 },
            Message::Pong { ts: 1 },
            Message::Error {
                code: "no_such_pane".into(),
                detail: "pane 9".into(),
                id: Some(2),
            },
            Message::Unsupported {
                t_of: "split_pane".into(),
            },
        ]
    }

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
}
