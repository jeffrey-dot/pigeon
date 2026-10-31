//! Rebuilding a layout from its description (`LayoutNode`, e.g. a tree saved before a host
//! restart), with the panes renumbered: pane ids are per process, so a restored tree gets
//! fresh ones.

use std::collections::HashMap;

use super::{Layout, LayoutNode, Node, PaneId, MIN_COLS, MIN_ROWS};
use crate::error::{PgError, PgResult};

impl Layout {
    /// The tree `described`, every leaf renamed through `renumber`, focus on `focus` (or the
    /// first pane). Ratios are kept as saved, clamped into (0, 1); a leaf `renumber` does not
    /// name, or a pane named twice, refuses the whole tree.
    pub fn from_described(
        described: &LayoutNode,
        renumber: &HashMap<PaneId, PaneId>,
        focus: Option<PaneId>,
    ) -> PgResult<Self> {
        let mut seen = Vec::new();
        let mut next_split = 1;
        let root = rebuild(described, renumber, &mut seen, &mut next_split)?;
        let focused = focus
            .and_then(|old| renumber.get(&old).copied())
            .filter(|pane| seen.contains(pane))
            .or_else(|| seen.first().copied())
            .ok_or(PgError::InvalidArg)?;
        Ok(Self {
            root: Some(root),
            focused,
            next_split,
            min_cols: MIN_COLS,
            min_rows: MIN_ROWS,
            zoom: None,
        })
    }
}

fn rebuild(
    node: &LayoutNode,
    renumber: &HashMap<PaneId, PaneId>,
    seen: &mut Vec<PaneId>,
    next_split: &mut u32,
) -> PgResult<Node> {
    match node {
        LayoutNode::Leaf(old) => {
            let pane = *renumber.get(old).ok_or(PgError::InvalidArg)?;
            if seen.contains(&pane) {
                return Err(PgError::InvalidArg);
            }
            seen.push(pane);
            Ok(Node::Leaf(pane))
        }
        LayoutNode::Split { axis, ratio, a, b, .. } => {
            let id = *next_split;
            *next_split = next_split.saturating_add(1);
            let ratio = if ratio.is_finite() { ratio.clamp(0.05, 0.95) } else { 0.5 };
            Ok(Node::Split {
                id,
                axis: *axis,
                ratio,
                a: Box::new(rebuild(a, renumber, seen, next_split)?),
                b: Box::new(rebuild(b, renumber, seen, next_split)?),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Axis;

    fn saved() -> LayoutNode {
        LayoutNode::Split {
            id: 7,
            axis: Axis::Vertical,
            ratio: 0.3,
            a: Box::new(LayoutNode::Leaf(4)),
            b: Box::new(LayoutNode::Leaf(9)),
        }
    }

    #[test]
    fn a_saved_tree_comes_back_with_new_pane_ids() {
        let renumber = HashMap::from([(4, 101), (9, 102)]);
        let layout = Layout::from_described(&saved(), &renumber, Some(9)).expect("rebuild");
        assert_eq!(layout.pane_ids(), vec![101, 102]);
        assert_eq!(layout.focused(), Some(102));
        let rects = layout.rects();
        assert!((rects[0].1.w - 0.3).abs() < 1e-6, "the ratio survives");
    }

    #[test]
    fn a_tree_that_does_not_match_its_panes_is_refused() {
        assert!(Layout::from_described(&saved(), &HashMap::from([(4, 101)]), None).is_err());
        let twice = HashMap::from([(4, 101), (9, 101)]);
        assert!(Layout::from_described(&saved(), &twice, None).is_err());
        // A bad ratio is repaired, a missing focus falls back to the first pane.
        let weird = LayoutNode::Split {
            id: 1,
            axis: Axis::Horizontal,
            ratio: f32::NAN,
            a: Box::new(LayoutNode::Leaf(4)),
            b: Box::new(LayoutNode::Leaf(9)),
        };
        let layout = Layout::from_described(&weird, &HashMap::from([(4, 1), (9, 2)]), Some(77))
            .expect("rebuild");
        assert_eq!(layout.focused(), Some(1));
    }
}
