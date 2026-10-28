//! Message codec for the C ABI.
//!
//! The Swift client never touches CBOR: it builds a [`PgMsg`] by field name,
//! asks the core to frame it, and reads replies back through the same handle.
//! Field names are exactly the ones in `docs/protocol.md`.
//!
//! This is deliberately a dynamic surface: one opaque handle plus getters and
//! setters, so adding a protocol field never changes the ABI. Schema checks still
//! happen in Rust — [`pg_msg_encode_frame`] round-trips through
//! [`proto::decode`], so a message with a wrong or missing field is rejected
//! here, not on the wire.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

use ciborium::value::Value;

use super::{ffi_call, PgAxis, PgRect, PgSnapshot, PgSplit, PgStr};
use crate::error::{PgError, PgResult};
use crate::proto::{self, Message, MAX_FRAME};

/// The M0 subset of `docs/protocol.md` §3.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgMsgType {
    PG_MSG_UNKNOWN = 0,
    PG_MSG_HELLO = 1,
    PG_MSG_HELLO_ACK = 2,
    PG_MSG_HELLO_ERROR = 3,
    PG_MSG_SESSIONS = 4,
    PG_MSG_NEW_SESSION = 5,
    PG_MSG_CREATED = 6,
    PG_MSG_CLOSE_PANE = 7,
    PG_MSG_ATTACH = 8,
    PG_MSG_SNAPSHOT = 9,
    PG_MSG_DELTA = 10,
    PG_MSG_KEYS = 11,
    PG_MSG_RESIZE_PANE = 12,
    PG_MSG_PANE_EXITED = 13,
    PG_MSG_PING = 14,
    PG_MSG_PONG = 15,
    PG_MSG_ERROR = 16,
    PG_MSG_UNSUPPORTED = 17,
    PG_MSG_SPLIT_PANE = 18,
    PG_MSG_RESIZE_SPLIT = 19,
    PG_MSG_FOCUS = 20,
    PG_MSG_LAYOUT = 21,
}

/// Wire name ↔ enum. Kept in one place so the two can never drift.
const NAMES: &[(PgMsgType, &str)] = &[
    (PgMsgType::PG_MSG_HELLO, "hello"),
    (PgMsgType::PG_MSG_HELLO_ACK, "hello_ack"),
    (PgMsgType::PG_MSG_HELLO_ERROR, "hello_error"),
    (PgMsgType::PG_MSG_SESSIONS, "sessions"),
    (PgMsgType::PG_MSG_NEW_SESSION, "new_session"),
    (PgMsgType::PG_MSG_CREATED, "created"),
    (PgMsgType::PG_MSG_CLOSE_PANE, "close_pane"),
    (PgMsgType::PG_MSG_ATTACH, "attach"),
    (PgMsgType::PG_MSG_SNAPSHOT, "snapshot"),
    (PgMsgType::PG_MSG_DELTA, "delta"),
    (PgMsgType::PG_MSG_KEYS, "keys"),
    (PgMsgType::PG_MSG_RESIZE_PANE, "resize_pane"),
    (PgMsgType::PG_MSG_PANE_EXITED, "pane_exited"),
    (PgMsgType::PG_MSG_PING, "ping"),
    (PgMsgType::PG_MSG_PONG, "pong"),
    (PgMsgType::PG_MSG_ERROR, "error"),
    (PgMsgType::PG_MSG_UNSUPPORTED, "unsupported"),
    (PgMsgType::PG_MSG_SPLIT_PANE, "split_pane"),
    (PgMsgType::PG_MSG_RESIZE_SPLIT, "resize_split"),
    (PgMsgType::PG_MSG_FOCUS, "focus"),
    (PgMsgType::PG_MSG_LAYOUT, "layout"),
];

fn name_of(msg_type: PgMsgType) -> Option<&'static str> {
    NAMES
        .iter()
        .find(|(candidate, _)| *candidate == msg_type)
        .map(|(_, name)| *name)
}

fn type_of(name: &str) -> PgMsgType {
    NAMES
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map_or(PgMsgType::PG_MSG_UNKNOWN, |(kind, _)| *kind)
}

/// Opaque message handle. Holds either a decoded message or a message being
/// built field by field.
pub struct PgMsg {
    t: String,
    fields: BTreeMap<String, Value>,
}

impl PgMsg {
    fn from_message(message: &Message) -> PgResult<Self> {
        let payload = proto::encode(message)?;
        let Value::Map(entries) =
            ciborium::from_reader::<Value, _>(&payload[..]).map_err(|_| PgError::InvalidArg)?
        else {
            return Err(PgError::InvalidArg);
        };
        let mut fields = BTreeMap::new();
        for (key, value) in entries {
            if let Value::Text(name) = key {
                if name != "t" {
                    fields.insert(name, value);
                }
            }
        }
        Ok(Self {
            t: message.name().to_owned(),
            fields,
        })
    }

    /// Rebuilds the typed message; used by the snapshot accessor so the wire
    /// schema is validated in exactly one place.
    fn to_message(&self) -> PgResult<Message> {
        let mut payload = Vec::new();
        ciborium::into_writer(&self.as_value(), &mut payload).map_err(|_| PgError::InvalidArg)?;
        proto::decode(&payload)
    }

    fn as_value(&self) -> Value {
        let mut entries = vec![(Value::Text("t".to_owned()), Value::Text(self.t.clone()))];
        for (name, value) in &self.fields {
            entries.push((Value::Text(name.clone()), value.clone()));
        }
        Value::Map(entries)
    }
}

/// Creates an empty message of `kind`, or NULL for `PG_MSG_UNKNOWN`.
#[no_mangle]
pub extern "C" fn pg_msg_new(kind: PgMsgType) -> *mut PgMsg {
    ffi_call(ptr::null_mut(), || {
        let name = name_of(kind).ok_or(PgError::InvalidArg)?;
        Ok(Box::into_raw(Box::new(PgMsg {
            t: name.to_owned(),
            fields: BTreeMap::new(),
        })))
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_free(msg: *mut PgMsg) {
    ffi_call((), || {
        if !msg.is_null() {
            // SAFETY: handle came from pg_msg_new / pg_msg_decode_frame.
            drop(unsafe { Box::from_raw(msg) });
        }
        Ok(())
    });
}

#[no_mangle]
pub extern "C" fn pg_msg_type(msg: *const PgMsg) -> PgMsgType {
    ffi_call(PgMsgType::PG_MSG_UNKNOWN, || {
        let msg = unsafe_ref(msg)?;
        Ok(type_of(&msg.t))
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_set_u32(msg: *mut PgMsg, field: *const c_char, value: u32) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        msg.fields
            .insert(field, Value::Integer(u64::from(value).into()));
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_set_i32(msg: *mut PgMsg, field: *const c_char, value: i32) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        msg.fields
            .insert(field, Value::Integer(i64::from(value).into()));
        Ok(true)
    })
}

/// Floats are only used for split ratios; they travel as CBOR float32/64.
#[no_mangle]
pub extern "C" fn pg_msg_set_f32(msg: *mut PgMsg, field: *const c_char, value: f32) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        msg.fields.insert(field, Value::Float(f64::from(value)));
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_get_f32(msg: *const PgMsg, field: *const c_char, fallback: f32) -> f32 {
    ffi_call(fallback, || {
        let msg = unsafe_ref(msg)?;
        let field = field_name(field)?;
        match msg.fields.get(&field) {
            Some(Value::Float(value)) => Ok(*value as f32),
            Some(Value::Integer(integer)) => Ok(i128::from(*integer) as f32),
            None => Ok(fallback),
            Some(_) => Err(PgError::InvalidArg),
        }
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_set_bool(msg: *mut PgMsg, field: *const c_char, value: bool) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        msg.fields.insert(field, Value::Bool(value));
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_set_str(msg: *mut PgMsg, field: *const c_char, value: PgStr) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        let text = pg_str_to_string(value)?;
        msg.fields.insert(field, Value::Text(text));
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_set_bytes(
    msg: *mut PgMsg,
    field: *const c_char,
    data: *const u8,
    len: usize,
) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        if data.is_null() && len > 0 {
            return Err(PgError::InvalidArg);
        }
        // SAFETY: caller promises `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        msg.fields.insert(field, Value::Bytes(bytes));
        Ok(true)
    })
}

/// Reads an unsigned field; `fallback` when absent or of the wrong type.
#[no_mangle]
pub extern "C" fn pg_msg_get_u32(msg: *const PgMsg, field: *const c_char, fallback: u32) -> u32 {
    ffi_call(fallback, || {
        let msg = unsafe_ref(msg)?;
        let field = field_name(field)?;
        let Some(value) = msg.fields.get(&field) else {
            return Ok(fallback);
        };
        let Value::Integer(integer) = value else {
            return Err(PgError::InvalidArg);
        };
        let raw: i128 = (*integer).into();
        Ok(u32::try_from(raw).unwrap_or(fallback))
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_get_i32(msg: *const PgMsg, field: *const c_char, fallback: i32) -> i32 {
    ffi_call(fallback, || {
        let msg = unsafe_ref(msg)?;
        let field = field_name(field)?;
        let Some(value) = msg.fields.get(&field) else {
            return Ok(fallback);
        };
        let Value::Integer(integer) = value else {
            return Err(PgError::InvalidArg);
        };
        let raw: i128 = (*integer).into();
        Ok(i32::try_from(raw).unwrap_or(fallback))
    })
}

#[no_mangle]
pub extern "C" fn pg_msg_get_bool(msg: *const PgMsg, field: *const c_char, fallback: bool) -> bool {
    ffi_call(fallback, || {
        let msg = unsafe_ref(msg)?;
        let field = field_name(field)?;
        match msg.fields.get(&field) {
            None => Ok(fallback),
            Some(Value::Bool(value)) => Ok(*value),
            Some(_) => Err(PgError::InvalidArg),
        }
    })
}

/// Borrows a text field. The bytes stay valid until `msg` is freed or mutated.
#[no_mangle]
pub extern "C" fn pg_msg_get_str(msg: *const PgMsg, field: *const c_char) -> PgStr {
    ffi_call(empty_str(), || {
        let msg = unsafe_ref(msg)?;
        let field = field_name(field)?;
        match msg.fields.get(&field) {
            Some(Value::Text(text)) => Ok(str_view(text)),
            None => Ok(empty_str()),
            Some(_) => Err(PgError::InvalidArg),
        }
    })
}

/// Bytes needed by [`pg_msg_encode_frame`], or 0 if the message is incomplete.
#[no_mangle]
pub extern "C" fn pg_msg_encoded_len(msg: *const PgMsg) -> usize {
    ffi_call(0, || {
        let msg = unsafe_ref(msg)?;
        Ok(encode_frame(msg)?.len())
    })
}

/// Writes one framed message into `out`; returns bytes written, 0 on failure
/// (`PG_ERR_OVERFLOW` when `cap` is too small).
#[no_mangle]
pub extern "C" fn pg_msg_encode_frame(msg: *const PgMsg, out: *mut u8, cap: usize) -> usize {
    ffi_call(0, || {
        let msg = unsafe_ref(msg)?;
        let frame = encode_frame(msg)?;
        if out.is_null() || cap < frame.len() {
            return Err(PgError::Overflow);
        }
        // SAFETY: caller provided `cap` writable bytes.
        unsafe { ptr::copy_nonoverlapping(frame.as_ptr(), out, frame.len()) };
        Ok(frame.len())
    })
}

/// Reads one frame from `buf`.
///
/// * returns a message and sets `*consumed` — one frame was read;
/// * returns NULL with `*consumed == 0` and `PG_OK` — need more bytes;
/// * returns NULL with an error code — the stream is unusable.
///
/// An unknown message type yields a `PG_MSG_UNSUPPORTED` handle carrying `t_of`,
/// so the caller can answer without dropping the connection.
#[no_mangle]
pub extern "C" fn pg_msg_decode_frame(
    buf: *const u8,
    len: usize,
    consumed: *mut usize,
) -> *mut PgMsg {
    ffi_call(ptr::null_mut(), || {
        if consumed.is_null() {
            return Err(PgError::InvalidArg);
        }
        // SAFETY: `consumed` is a valid out pointer.
        unsafe { *consumed = 0 };
        if len < 4 {
            return Ok(ptr::null_mut());
        }
        if buf.is_null() {
            return Err(PgError::InvalidArg);
        }
        // SAFETY: caller promises `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(buf, len) };
        let length = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        if length > MAX_FRAME {
            return Err(PgError::Overflow);
        }
        if bytes.len() < 4 + length {
            return Ok(ptr::null_mut());
        }
        let total = 4 + length;
        let payload = &bytes[4..total];
        let message = match proto::decode(payload) {
            Ok(message) => message,
            Err(PgError::Unsupported) => {
                let tag = type_tag(payload).unwrap_or_default();
                let mut handle = PgMsg {
                    t: "unsupported".to_owned(),
                    fields: BTreeMap::new(),
                };
                handle.fields.insert("t_of".to_owned(), Value::Text(tag));
                // SAFETY: out pointer checked above.
                unsafe { *consumed = total };
                return Ok(Box::into_raw(Box::new(handle)));
            }
            Err(err) => return Err(err),
        };
        let handle = PgMsg::from_message(&message)?;
        // SAFETY: out pointer checked above.
        unsafe { *consumed = total };
        Ok(Box::into_raw(Box::new(handle)))
    })
}

/// Builds an owned snapshot from a `snapshot` message; free with
/// [`super::pg_snapshot_free`]. NULL for any other message type.
///
/// Goes through [`crate::session::snapshot_from_message`], so cell widths (wide
/// chars and their continuation) are derived exactly like everywhere else.
#[no_mangle]
pub extern "C" fn pg_msg_snapshot(msg: *const PgMsg) -> *mut PgSnapshot {
    ffi_call(ptr::null_mut(), || {
        let msg = unsafe_ref(msg)?;
        if type_of(&msg.t) != PgMsgType::PG_MSG_SNAPSHOT {
            return Err(PgError::InvalidArg);
        }
        let message = msg.to_message()?;
        let pane_id = match message {
            Message::Snapshot { pane_id, .. } => pane_id,
            _ => return Err(PgError::InvalidArg),
        };
        let snapshot =
            crate::session::snapshot_from_message(&message).ok_or(PgError::InvalidArg)?;
        Ok(super::snapshot_to_ffi(pane_id, &snapshot, true))
    })
}

/// Payload-only codec, exactly the names the contract asks for: CBOR bytes in and
/// out, without the 4-byte frame prefix.
#[no_mangle]
pub extern "C" fn pg_msg_encode(msg: *const PgMsg, out: *mut u8, cap: usize) -> usize {
    ffi_call(0, || {
        let msg = unsafe_ref(msg)?;
        let payload = encode_payload(msg)?;
        if out.is_null() || cap < payload.len() {
            return Err(PgError::Overflow);
        }
        // SAFETY: caller provided `cap` writable bytes.
        unsafe { ptr::copy_nonoverlapping(payload.as_ptr(), out, payload.len()) };
        Ok(payload.len())
    })
}

/// Decodes one CBOR payload (no length prefix). Unknown `t` yields a handle of
/// type `PG_MSG_UNSUPPORTED` carrying `t_of`.
#[no_mangle]
pub extern "C" fn pg_msg_decode(payload: *const u8, len: usize) -> *mut PgMsg {
    ffi_call(ptr::null_mut(), || {
        if payload.is_null() && len > 0 {
            return Err(PgError::InvalidArg);
        }
        // SAFETY: caller promises `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(payload, len) };
        match proto::decode(bytes) {
            Ok(message) => Ok(Box::into_raw(Box::new(PgMsg::from_message(&message)?))),
            Err(PgError::Unsupported) => {
                let tag = type_tag(bytes).unwrap_or_default();
                let handle = PgMsg {
                    t: "unsupported".to_owned(),
                    fields: BTreeMap::from([("t_of".to_owned(), Value::Text(tag))]),
                };
                Ok(Box::into_raw(Box::new(handle)))
            }
            Err(err) => Err(err),
        }
    })
}

/// Rects of a `layout` message, computed by the core (clients never do the maths).
#[no_mangle]
pub extern "C" fn pg_msg_layout_rects(msg: *const PgMsg, out: *mut PgRect, cap: usize) -> usize {
    ffi_call(0, || {
        let node = layout_node(unsafe_ref(msg)?)?;
        let rects = node.rects();
        if rects.is_empty() {
            return Ok(0);
        }
        if out.is_null() || cap < rects.len() {
            return Err(PgError::Overflow);
        }
        for (index, (pane_id, rect)) in rects.iter().enumerate() {
            // SAFETY: the caller promised `cap` writable entries.
            unsafe {
                *out.add(index) = PgRect {
                    size: std::mem::size_of::<PgRect>() as u32,
                    pane_id: *pane_id,
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: rect.h,
                };
            }
        }
        Ok(rects.len())
    })
}

/// Dividers of a `layout` message, for divider hit-testing and drawing.
#[no_mangle]
pub extern "C" fn pg_msg_layout_splits(msg: *const PgMsg, out: *mut PgSplit, cap: usize) -> usize {
    ffi_call(0, || {
        let node = layout_node(unsafe_ref(msg)?)?;
        let splits = node.splits();
        if splits.is_empty() {
            return Ok(0);
        }
        if out.is_null() || cap < splits.len() {
            return Err(PgError::Overflow);
        }
        for (index, (id, axis, ratio, rect)) in splits.iter().enumerate() {
            // SAFETY: the caller promised `cap` writable entries.
            unsafe {
                *out.add(index) = PgSplit {
                    size: std::mem::size_of::<PgSplit>() as u32,
                    id: *id,
                    axis: match axis {
                        crate::layout::Axis::Vertical => PgAxis::PG_AXIS_VERTICAL,
                        crate::layout::Axis::Horizontal => PgAxis::PG_AXIS_HORIZONTAL,
                    },
                    ratio: *ratio,
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: rect.h,
                };
            }
        }
        Ok(splits.len())
    })
}

/// Focus carried by a `layout` message (0 when absent).
#[no_mangle]
pub extern "C" fn pg_msg_layout_focus(msg: *const PgMsg) -> u32 {
    ffi_call(0, || {
        let msg = unsafe_ref(msg)?;
        if type_of(&msg.t) != PgMsgType::PG_MSG_LAYOUT {
            return Err(PgError::InvalidArg);
        }
        Ok(match msg.to_message()? {
            Message::Layout { focus, .. } => focus.unwrap_or(0),
            _ => 0,
        })
    })
}

fn layout_node(msg: &PgMsg) -> PgResult<crate::layout::LayoutNode> {
    if type_of(&msg.t) != PgMsgType::PG_MSG_LAYOUT {
        return Err(PgError::InvalidArg);
    }
    match msg.to_message()? {
        Message::Layout { tree, .. } => Ok(from_proto_node(&tree)),
        _ => Err(PgError::InvalidArg),
    }
}

fn from_proto_node(node: &crate::proto::Node) -> crate::layout::LayoutNode {
    use crate::layout::{Axis as LayoutAxis, LayoutNode};
    use crate::proto::Node;
    match node {
        Node::Leaf { leaf } => LayoutNode::Leaf(*leaf),
        Node::Split { split } => LayoutNode::Split {
            id: split.id,
            axis: match split.axis {
                crate::proto::SplitAxis::Vertical => LayoutAxis::Vertical,
                crate::proto::SplitAxis::Horizontal => LayoutAxis::Horizontal,
            },
            ratio: split.ratio,
            a: Box::new(from_proto_node(&split.a)),
            b: Box::new(from_proto_node(&split.b)),
        },
    }
}

fn encode_payload(msg: &PgMsg) -> PgResult<Vec<u8>> {
    let message = msg.to_message()?;
    proto::encode(&message)
}

// ---------------------------------------------------------------- helpers

fn encode_frame(msg: &PgMsg) -> PgResult<Vec<u8>> {
    // Validate against the real schema before anything reaches the wire.
    let message = msg.to_message()?;
    proto::encode_frame(&message)
}

fn type_tag(payload: &[u8]) -> Option<String> {
    let Value::Map(entries) = ciborium::from_reader::<Value, _>(payload).ok()? else {
        return None;
    };
    entries
        .into_iter()
        .find_map(|(key, value)| match (key, value) {
            (Value::Text(name), Value::Text(tag)) if name == "t" => Some(tag),
            _ => None,
        })
}

fn field_name(field: *const c_char) -> PgResult<String> {
    if field.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller passes a NUL-terminated string.
    unsafe { CStr::from_ptr(field) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| PgError::InvalidArg)
}

fn pg_str_to_string(value: PgStr) -> PgResult<String> {
    if value.ptr.is_null() {
        return if value.len == 0 {
            Ok(String::new())
        } else {
            Err(PgError::InvalidArg)
        };
    }
    // SAFETY: caller promises `len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    String::from_utf8(bytes.to_vec()).map_err(|_| PgError::InvalidArg)
}

fn str_view(text: &str) -> PgStr {
    PgStr {
        size: std::mem::size_of::<PgStr>() as u32,
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

fn empty_str() -> PgStr {
    PgStr {
        size: std::mem::size_of::<PgStr>() as u32,
        ptr: b"".as_ptr(),
        len: 0,
    }
}

/// # Safety
/// `msg` must be null or a live handle from this module.
fn unsafe_ref<'a>(msg: *const PgMsg) -> PgResult<&'a PgMsg> {
    if msg.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: guaranteed by the caller per the ABI contract.
    Ok(unsafe { &*msg })
}

/// # Safety
/// `msg` must be null or a live handle from this module.
fn unsafe_mut<'a>(msg: *mut PgMsg) -> PgResult<&'a mut PgMsg> {
    if msg.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: guaranteed by the caller per the ABI contract.
    Ok(unsafe { &mut *msg })
}

/// One entry of a `sessions` message. Strings are borrowed from the message.
#[repr(C)]
pub struct PgSessionInfo {
    pub size: u32,
    pub session_id: u32,
    pub title: PgStr,
    pub cwd: PgStr,
    pub pane_count: u32,
}

/// Sets a field to an array of strings (e.g. `hello.features`).
#[no_mangle]
pub extern "C" fn pg_msg_set_str_array(
    msg: *mut PgMsg,
    field: *const c_char,
    values: *const PgStr,
    count: usize,
) -> bool {
    ffi_call(false, || {
        let msg = unsafe_mut(msg)?;
        let field = field_name(field)?;
        if values.is_null() && count > 0 {
            return Err(PgError::InvalidArg);
        }
        let mut items = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: caller promises `count` readable entries.
            let value = unsafe { *values.add(index) };
            items.push(Value::Text(pg_str_to_string(value)?));
        }
        msg.fields.insert(field, Value::Array(items));
        Ok(true)
    })
}

/// Number of entries in a `sessions` message.
#[no_mangle]
pub extern "C" fn pg_msg_sessions_count(msg: *const PgMsg) -> usize {
    ffi_call(0, || {
        let msg = unsafe_ref(msg)?;
        match msg.fields.get("items") {
            Some(Value::Array(items)) => Ok(items.len()),
            _ => Err(PgError::InvalidArg),
        }
    })
}

/// Fills `out` with one session entry; returns false when `index` is out of range.
#[no_mangle]
pub extern "C" fn pg_msg_session_at(
    msg: *const PgMsg,
    index: usize,
    out: *mut PgSessionInfo,
) -> bool {
    ffi_call(false, || {
        let msg = unsafe_ref(msg)?;
        if out.is_null() {
            return Err(PgError::InvalidArg);
        }
        let Some(Value::Map(entries)) = session_entry(msg, index)? else {
            return Err(PgError::InvalidArg);
        };
        let mut info = PgSessionInfo {
            size: std::mem::size_of::<PgSessionInfo>() as u32,
            session_id: 0,
            title: empty_str(),
            cwd: empty_str(),
            pane_count: 0,
        };
        for (key, value) in entries {
            let Value::Text(name) = key else { continue };
            match (name.as_str(), value) {
                ("session_id", Value::Integer(number)) => {
                    info.session_id = u32::try_from(i128::from(*number)).unwrap_or(0);
                }
                ("title", Value::Text(text)) => info.title = str_view(text),
                ("cwd", Value::Text(text)) => info.cwd = str_view(text),
                ("panes", Value::Array(panes)) => {
                    info.pane_count = panes.len() as u32;
                }
                _ => {}
            }
        }
        // SAFETY: checked non-null above.
        unsafe { *out = info };
        Ok(true)
    })
}

/// Copies the pane ids of session `index` into `out`; returns how many were written.
#[no_mangle]
pub extern "C" fn pg_msg_session_panes(
    msg: *const PgMsg,
    index: usize,
    out: *mut u32,
    cap: usize,
) -> usize {
    ffi_call(0, || {
        let msg = unsafe_ref(msg)?;
        let Some(Value::Map(entries)) = session_entry(msg, index)? else {
            return Err(PgError::InvalidArg);
        };
        let Some((_, Value::Array(panes))) = entries
            .iter()
            .find(|(key, _)| matches!(key, Value::Text(name) if name == "panes"))
        else {
            return Err(PgError::InvalidArg);
        };
        if out.is_null() && !panes.is_empty() {
            return Err(PgError::InvalidArg);
        }
        let mut written = 0;
        for pane in panes {
            if written == cap {
                break;
            }
            let Value::Integer(number) = pane else {
                continue;
            };
            let Ok(id) = u32::try_from(i128::from(*number)) else {
                continue;
            };
            // SAFETY: caller promised `cap` writable entries.
            unsafe { *out.add(written) = id };
            written += 1;
        }
        Ok(written)
    })
}

fn session_entry(msg: &PgMsg, index: usize) -> PgResult<Option<&Value>> {
    let Some(Value::Array(items)) = msg.fields.get("items") else {
        return Err(PgError::InvalidArg);
    };
    Ok(items.get(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{
        pg_last_error_code, pg_snapshot_free, PgErrorCode, PgMsgType, PgStatus, PG_CORE_ABI_VERSION,
    };
    use std::ffi::CString;

    fn field(name: &str) -> CString {
        CString::new(name).expect("field name")
    }

    fn text(value: &str) -> PgStr {
        str_view(value)
    }

    #[test]
    fn unknown_type_cannot_be_created() {
        let msg = pg_msg_new(PgMsgType::PG_MSG_UNKNOWN);
        assert!(msg.is_null());
        assert_eq!(pg_last_error_code(), PgErrorCode::PG_ERR_INVALID_ARG);
    }

    #[test]
    fn outbound_hello_round_trips_through_the_abi() {
        let features = [text("scrollback"), text("osc7501")];
        let msg = pg_msg_new(PgMsgType::PG_MSG_HELLO);
        assert!(!msg.is_null());
        assert!(pg_msg_set_u32(
            msg,
            field("proto").as_ptr(),
            u32::from(proto::PROTO_VERSION)
        ));
        assert!(pg_msg_set_str(
            msg,
            field("client").as_ptr(),
            text("pigeon-mac")
        ));
        assert!(pg_msg_set_str_array(
            msg,
            field("features").as_ptr(),
            features.as_ptr(),
            features.len()
        ));

        let len = pg_msg_encoded_len(msg);
        assert!(len > 4);
        let mut frame = vec![0u8; len];
        assert_eq!(
            pg_msg_encode_frame(msg, frame.as_mut_ptr(), frame.len()),
            len
        );
        pg_msg_free(msg);

        let mut consumed = 0usize;
        let decoded = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
        assert!(!decoded.is_null(), "a complete hello must decode");
        assert_eq!(pg_msg_type(decoded), PgMsgType::PG_MSG_HELLO);
        pg_msg_free(decoded);
    }

    #[test]
    fn keys_message_round_trips() {
        let msg = pg_msg_new(PgMsgType::PG_MSG_KEYS);
        assert!(!msg.is_null());
        assert!(pg_msg_set_u32(msg, field("pane_id").as_ptr(), 3));
        let bytes = [0x1b, b'[', b'A'];
        assert!(pg_msg_set_bytes(
            msg,
            field("bytes").as_ptr(),
            bytes.as_ptr(),
            bytes.len()
        ));

        let len = pg_msg_encoded_len(msg);
        assert!(len > 4, "framed length includes the prefix");

        let mut frame = vec![0u8; len];
        assert_eq!(
            pg_msg_encode_frame(msg, frame.as_mut_ptr(), frame.len()),
            len
        );
        pg_msg_free(msg);

        let mut consumed = 0usize;
        let decoded = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
        assert!(!decoded.is_null());
        assert_eq!(consumed, len);
        assert_eq!(pg_msg_type(decoded), PgMsgType::PG_MSG_KEYS);
        assert_eq!(pg_msg_get_u32(decoded, field("pane_id").as_ptr(), 0), 3);
        pg_msg_free(decoded);
    }

    #[test]
    fn incomplete_frame_asks_for_more_without_an_error() {
        let msg = pg_msg_new(PgMsgType::PG_MSG_PING);
        assert!(pg_msg_set_u32(msg, field("ts").as_ptr(), 5));
        let len = pg_msg_encoded_len(msg);
        let mut frame = vec![0u8; len];
        assert_eq!(
            pg_msg_encode_frame(msg, frame.as_mut_ptr(), frame.len()),
            len
        );
        pg_msg_free(msg);

        let mut consumed = 1usize;
        let decoded = pg_msg_decode_frame(frame.as_ptr(), 2, &mut consumed);
        assert!(decoded.is_null());
        assert_eq!(consumed, 0);
        assert_eq!(pg_last_error_code(), PgErrorCode::PG_OK);
    }

    #[test]
    fn unknown_message_type_decodes_as_unsupported() {
        let payload = ciborium::value::Value::Map(vec![(
            Value::Text("t".to_owned()),
            Value::Text("definitely_not_a_message".to_owned()),
        )]);
        let mut body = Vec::new();
        ciborium::into_writer(&payload, &mut body).expect("encode");
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(&body);

        let mut consumed = 0usize;
        let decoded = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
        assert!(!decoded.is_null());
        assert_eq!(pg_msg_type(decoded), PgMsgType::PG_MSG_UNSUPPORTED);
        assert_eq!(
            pg_msg_get_str(decoded, field("t_of").as_ptr()).len,
            "definitely_not_a_message".len()
        );
        pg_msg_free(decoded);
    }

    #[test]
    fn snapshot_message_becomes_an_owned_pg_snapshot() {
        let cell = crate::proto::Cell {
            codepoint: 'A' as u32,
            attrs: 0,
            fg: 0x00ff_0000,
            bg: 0x0000_00ff,
            link: None,
        };
        let snapshot = Message::Snapshot {
            pane_id: 1,
            seq: 4,
            cols: 1,
            rows: 1,
            alt: false,
            cursor: proto::Cursor {
                x: 0,
                y: 0,
                visible: true,
                shape: 0,
            },
            scroll: proto::Scroll { top: 0, bottom: 0 },
            screen: vec![vec![cell]],
            history: None,
            links: None,
        };
        let frame = proto::encode_frame(&snapshot).expect("frame");
        let mut consumed = 0usize;
        let decoded = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut consumed);
        assert!(!decoded.is_null());

        let snap = pg_msg_snapshot(decoded);
        assert!(!snap.is_null());
        // SAFETY: just built, still owned here.
        let view = unsafe { &*snap };
        assert_eq!((view.cols, view.rows), (1, 1));
        assert_eq!(view.pane_id, 1);
        let row = unsafe { &*view.screen };
        let cell = unsafe { &*row.cells };
        assert_eq!(cell.codepoint, 'A' as u32);
        assert_eq!(cell.fg, 0x00ff_0000);
        pg_snapshot_free(snap);
        pg_msg_free(decoded);
    }

    #[test]
    fn version_constant_matches_the_header() {
        assert_eq!(PG_CORE_ABI_VERSION, 1);
        let _ = std::mem::size_of::<PgStatus>();
    }
}
