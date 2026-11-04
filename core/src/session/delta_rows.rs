//! The terminal picture carried by a protocol delta, shared by both client interfaces.

use crate::term::{Cell, Cursor};

/// Changed rows and optional state from one pane's `delta` frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeltaRows {
    pub seq: u32,
    pub alt: Option<bool>,
    /// `(y, cells)` in ascending order; every row has the same nonzero width.
    pub rows: Vec<(u16, Vec<Cell>)>,
    pub cursor: Option<Cursor>,
    /// Missing modes leave the client's previous mode bits unchanged.
    pub modes: Option<u32>,
}
