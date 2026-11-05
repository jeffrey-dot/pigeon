//! Owned link table used by both client interfaces; cells retain URI ownership in Rust.
use super::{Cell, Link};

/// Collects the URI table belonging to exactly these cells. IDs are meaningful only
/// alongside their owning snapshot/delta; resolve a URI before storing cells in a client.
pub fn cell_links<'a>(cells: impl Iterator<Item = &'a Cell>) -> Vec<Link> {
    crate::session::links::collect(cells).unwrap_or_default()
}
