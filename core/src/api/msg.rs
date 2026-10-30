//! Message helpers: the typed counterparts of `pg_msg_snapshot` and
//! `pg_msg_layout_*`, so Rust clients never redo the maths on a wire message.

use crate::error::{PgError, PgResult};
use crate::layout::{Axis, LayoutNode, PaneId, Rect, SplitId};
use crate::proto::{Message, Node, SplitAxis};
use crate::term::Snapshot;

/// A divider in a layout message: the box it divides plus its ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Split {
    pub id: SplitId,
    pub axis: Axis,
    pub ratio: f32,
    pub rect: Rect,
}

/// The pane id and terminal picture carried by a `snapshot` message.
///
/// The wire does not carry cell widths; they are re-derived from the
/// codepoints exactly like everywhere else. Malformed grids are refused.
pub fn snapshot_of(message: &Message) -> PgResult<(PaneId, Snapshot)> {
    let Message::Snapshot { pane_id, .. } = message else {
        return Err(PgError::InvalidArg);
    };
    let snapshot = crate::session::snapshot_from_message(message).ok_or(PgError::InvalidArg)?;
    Ok((*pane_id, snapshot))
}

/// Rects of a `layout` message, in tree order.
pub fn layout_rects(message: &Message) -> PgResult<Vec<(PaneId, Rect)>> {
    Ok(layout_node(message)?.rects())
}

/// Dividers of a `layout` message, for hit-testing and drawing.
pub fn layout_splits(message: &Message) -> PgResult<Vec<Split>> {
    Ok(layout_node(message)?
        .splits()
        .into_iter()
        .map(|(id, axis, ratio, rect)| Split {
            id,
            axis,
            ratio,
            rect,
        })
        .collect())
}

/// Focus carried by a `layout` message, if any.
pub fn layout_focus(message: &Message) -> PgResult<Option<PaneId>> {
    match message {
        Message::Layout { focus, .. } => Ok(*focus),
        _ => Err(PgError::InvalidArg),
    }
}

fn layout_node(message: &Message) -> PgResult<LayoutNode> {
    match message {
        Message::Layout { tree, .. } => Ok(from_proto_node(tree)),
        _ => Err(PgError::InvalidArg),
    }
}

fn from_proto_node(node: &Node) -> LayoutNode {
    match node {
        Node::Leaf { leaf } => LayoutNode::Leaf(*leaf),
        Node::Split { split } => LayoutNode::Split {
            id: split.id,
            axis: match split.axis {
                SplitAxis::Vertical => Axis::Vertical,
                SplitAxis::Horizontal => Axis::Horizontal,
            },
            ratio: split.ratio,
            a: Box::new(from_proto_node(&split.a)),
            b: Box::new(from_proto_node(&split.b)),
        },
    }
}
