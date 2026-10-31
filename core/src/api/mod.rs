//! The safe Rust client interface: one-to-one with the C ABI in [`crate::ffi`],
//! minus the pointers (`docs/core.md` §4.1).
//!
//! A Rust client depends on this module only. `ffi` is a thin wrapper around
//! it, so the two faces cannot drift apart. The internal modules (`term`,
//! `layout`, `status`, `session`, `proto`) are private unless the `internals`
//! feature is on; clients build with `default-features = false`, which keeps
//! them out of reach at compile time.
//!
//! Without `internals`, reaching past this module does not compile:
//!
#![cfg_attr(not(feature = "internals"), doc = "```compile_fail")]
#![cfg_attr(feature = "internals", doc = "```ignore")]
//! let _ = pigeon_core::term::ATTR_BOLD;
//! ```

mod core;
mod msg;

pub use self::core::{Config, Core};
pub use self::msg::{layout_focus, layout_rects, layout_splits, snapshot_of, Split};

pub use crate::error::{PgError, PgResult};
pub use crate::layout::{Axis, PaneId, Rect, SplitId};
pub use crate::proto::{
    decode, decode_frame, encode, encode_frame, Cell as WireCell, Cursor as WireCursor, Link,
    Message, Node, Row, Scroll, SessionInfo, SplitAxis, SplitNode, MAX_FRAME, PROTO_VERSION,
};
pub use crate::status::{Kind as StatusKind, Record as StatusRecord, State as StatusState};
pub use crate::term::{
    Cell, Cursor, Snapshot, ATTR_BOLD, ATTR_DIM, ATTR_ITALIC, ATTR_REVERSE, ATTR_STRIKETHROUGH,
    ATTR_UNDERLINE,
};
