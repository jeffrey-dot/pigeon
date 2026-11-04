//! Wire protocol: `u32` little-endian length prefix + one CBOR map.
//!
//! The single implementation of framing and encoding in the repository. The
//! client reaches it through the C ABI, never by re-implementing CBOR.
//!
//! Contract: `docs/protocol.md` §1–§4. M0 implements the subset listed in
//! [`Message`]; anything else decodes to [`PgError::Unsupported`] so the host can
//! answer `unsupported` without dropping the connection.

use std::fmt;

use ciborium::value::Value;
use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{PgError, PgResult};

mod message;
pub use message::Message;
mod status_snapshot;
#[cfg(feature = "internals")]
pub use status_snapshot::message as status_snapshot_message;
pub use status_snapshot::{snapshot_of as status_snapshot_of, WireStatusRecord};
mod cell_text;
pub use cell_text::{CellText, MAX_CELL_TEXT_BYTES};

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
    /// What to *call* that pane in the sidebar: the title when it names a program, the shell
    /// otherwise (fish titles its window `~/dev`, which is a path, not an app).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_program: Option<String>,
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

/// Serializes one message into its CBOR payload (no length prefix).
pub fn encode(message: &Message) -> PgResult<Vec<u8>> {
    status_snapshot::validate(message)?;
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
    let message = value
        .deserialized::<Message>()
        .map_err(|_| PgError::InvalidArg)?;
    status_snapshot::validate(&message)?;
    Ok(message)
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
    let Some((payload, used)) = split_frame(buf)? else {
        return Ok(None);
    };
    Ok(Some((decode(payload)?, used)))
}

/// The payload of the frame at the front of `buf` and the bytes that frame takes, without
/// decoding it: a reader that gets [`PgError::Unsupported`] from [`decode`] still knows
/// exactly how much to skip (protocol §2 rule 7: an unknown `t` costs one frame).
pub fn split_frame(buf: &[u8]) -> PgResult<Option<(&[u8], usize)>> {
    let Some(header) = buf.get(..4) else {
        return Ok(None);
    };
    let length = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    if length > MAX_FRAME {
        return Err(PgError::Overflow);
    }
    Ok(buf.get(4..4 + length).map(|payload| (payload, 4 + length)))
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
            | "paste"
            | "resize_pane"
            | "scroll"
            | "zoom_pane"
            | "unzoom"
            | "size_state"
            | "pane_exited"
            | "status"
            | "status_snapshot"
            | "ack_status"
            | "ping"
            | "shutdown"
            | "pong"
            | "error"
            | "unsupported"
    )
}

#[cfg(test)]
mod tests;
