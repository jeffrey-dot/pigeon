//! Layout tree: nested vsplit/hsplit, ratios and minimum sizes.
//!
//! Pure data. The host and every client read the same rects from here; nobody
//! else computes fractions or clamps minimum sizes.
//!
//! Vocabulary follows the protocol: a **vertical** split puts panes side by side
//! (a vertical divider), a **horizontal** split stacks them.

use crate::error::{PgError, PgResult};

pub type PaneId = u32;
pub type SplitId = u32;

/// Default minimum size of a pane, in cells.
pub const MIN_COLS: u16 = 10;
pub const MIN_ROWS: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Panes side by side, vertical divider (`"v"` in the protocol).
    Vertical,
    /// Panes stacked, horizontal divider (`"h"`).
    Horizontal,
}

/// Normalized rect: `0..=1` relative to the session box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

enum Node {
    Leaf(PaneId),
    Split {
        id: SplitId,
        axis: Axis,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

pub struct Layout {
    root: Option<Node>,
    focused: PaneId,
    next_split: SplitId,
    min_cols: u16,
    min_rows: u16,
}

impl Layout {
    pub fn new(pane: PaneId) -> Self {
        Self {
            root: Some(Node::Leaf(pane)),
            focused: pane,
            next_split: 1,
            min_cols: MIN_COLS,
            min_rows: MIN_ROWS,
        }
    }

    pub fn with_minimums(mut self, min_cols: u16, min_rows: u16) -> Self {
        self.min_cols = min_cols.max(2);
        self.min_rows = min_rows.max(1);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Panes in tree order: left-to-right, top-to-bottom.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            collect_panes(root, &mut out);
        }
        out
    }

    pub fn focused(&self) -> Option<PaneId> {
        self.root.as_ref().map(|_| self.focused)
    }

    /// Splits `target` and gives the new half to `pane`; returns the new split id.
    pub fn split(
        &mut self,
        target: PaneId,
        pane: PaneId,
        axis: Axis,
        ratio: f32,
        cols: u16,
        rows: u16,
    ) -> PgResult<SplitId> {
        // Caller passes a fresh id; duplicates would corrupt the tree.
        if self.pane_ids().contains(&pane) {
            return Err(PgError::InvalidArg);
        }
        let root = self.root.take().ok_or(PgError::NoSuchPane)?;
        let (root, split_id) = self.split_in(root, target, pane, axis, ratio, cols, rows)?;
        self.root = Some(root);
        if split_id == 0 {
            // Nothing matched `target`; the tree is unchanged.
            return Err(PgError::NoSuchPane);
        }
        Ok(split_id)
    }

    // Internal recursion carries the same split parameters as `split`.
    #[allow(clippy::too_many_arguments)]
    fn split_in(
        &mut self,
        node: Node,
        target: PaneId,
        pane: PaneId,
        axis: Axis,
        ratio: f32,
        cols: u16,
        rows: u16,
    ) -> PgResult<(Node, SplitId)> {
        match node {
            Node::Leaf(id) if id == target => {
                let split_id = self.next_split;
                self.next_split = self.next_split.saturating_add(1);
                let ratio = clamp_ratio(ratio, axis, cols, rows, self.min_cols, self.min_rows);
                Ok((
                    Node::Split {
                        id: split_id,
                        axis,
                        ratio,
                        a: Box::new(Node::Leaf(id)),
                        b: Box::new(Node::Leaf(pane)),
                    },
                    split_id,
                ))
            }
            Node::Leaf(id) => Ok((Node::Leaf(id), 0)),
            Node::Split {
                id,
                axis: node_axis,
                ratio: node_ratio,
                a,
                b,
            } => {
                let a = *a;
                let b = *b;
                let (new_a, found) = self.split_in(a, target, pane, axis, ratio, cols, rows)?;
                if found != 0 {
                    return Ok((
                        Node::Split {
                            id,
                            axis: node_axis,
                            ratio: node_ratio,
                            a: Box::new(new_a),
                            b: Box::new(b),
                        },
                        found,
                    ));
                }
                let (new_b, found) = self.split_in(b, target, pane, axis, ratio, cols, rows)?;
                Ok((
                    Node::Split {
                        id,
                        axis: node_axis,
                        ratio: node_ratio,
                        a: Box::new(new_a),
                        b: Box::new(new_b),
                    },
                    found,
                ))
            }
        }
    }

    /// Removes a pane; its sibling takes over the parent's rect.
    pub fn close(&mut self, pane: PaneId) -> PgResult<()> {
        let root = self.root.take().ok_or(PgError::NoSuchPane)?;
        let (root, found) = remove(root, pane);
        self.root = root;
        if !found {
            return Err(PgError::NoSuchPane);
        }
        let remaining = self.pane_ids();
        if !remaining.contains(&self.focused) {
            self.focused = remaining.first().copied().unwrap_or(0);
        }
        Ok(())
    }

    /// Changes a split's ratio, clamped so both sides keep their minimum size.
    pub fn set_ratio(
        &mut self,
        split_id: SplitId,
        ratio: f32,
        cols: u16,
        rows: u16,
    ) -> PgResult<()> {
        let root = self.root.as_mut().ok_or(PgError::NoSuchSplit)?;
        let min_cols = self.min_cols;
        let min_rows = self.min_rows;
        resize_in(root, split_id, ratio, cols, rows, min_cols, min_rows).ok_or(PgError::NoSuchSplit)
    }

    /// Normalized rects for every pane, in tree order.
    pub fn rects(&self) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            collect_rects(
                root,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                &mut out,
            );
        }
        out
    }

    /// Whether this tree contains `split_id`.
    pub fn split_of_any(&self, split_id: SplitId) -> bool {
        self.root
            .as_ref()
            .is_some_and(|root| has_split(root, split_id))
    }

    /// A public mirror of the tree, for hosts that need to describe it.
    pub fn describe(&self) -> Option<LayoutNode> {
        self.root.as_ref().map(describe_node)
    }

    /// The deepest split on the path to `pane` — the divider that resizes it.
    pub fn split_of(&self, pane: PaneId) -> Option<SplitId> {
        self.root
            .as_ref()
            .and_then(|root| deepest_split(root, pane, None))
    }

    pub fn focus(&mut self, pane: PaneId) -> bool {
        if self.pane_ids().contains(&pane) {
            self.focused = pane;
            true
        } else {
            false
        }
    }

    /// Moves focus to the next/previous pane in tree order (wraps).
    pub fn focus_next(&mut self, forward: bool) -> Option<PaneId> {
        let panes = self.pane_ids();
        if panes.is_empty() {
            return None;
        }
        let index = panes.iter().position(|id| *id == self.focused).unwrap_or(0);
        let next = if forward {
            (index + 1) % panes.len()
        } else {
            (index + panes.len() - 1) % panes.len()
        };
        self.focused = panes[next];
        Some(self.focused)
    }
}

fn clamp_ratio(ratio: f32, axis: Axis, cols: u16, rows: u16, min_cols: u16, min_rows: u16) -> f32 {
    let (total, minimum) = match axis {
        Axis::Vertical => (f32::from(cols), f32::from(min_cols)),
        Axis::Horizontal => (f32::from(rows), f32::from(min_rows)),
    };
    if total <= 0.0 {
        return 0.5;
    }
    let min_fraction = (minimum / total).min(0.5);
    ratio.clamp(min_fraction, 1.0 - min_fraction)
}

pub enum LayoutNode {
    Leaf(PaneId),
    Split {
        id: SplitId,
        axis: Axis,
        ratio: f32,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

impl LayoutNode {
    /// Normalized rects in tree order — the geometry the client draws.
    pub fn rects(&self) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.collect_rects(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            &mut out,
        );
        out
    }

    /// Every divider: `(id, axis, ratio, the box it divides)`.
    pub fn splits(&self) -> Vec<(SplitId, Axis, f32, Rect)> {
        let mut out = Vec::new();
        self.collect_splits(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            &mut out,
        );
        out
    }

    fn collect_rects(&self, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Leaf(pane) => out.push((*pane, rect)),
            Self::Split {
                axis, ratio, a, b, ..
            } => {
                let (first, second) = split_rects(rect, *axis, *ratio);
                a.collect_rects(first, out);
                b.collect_rects(second, out);
            }
        }
    }

    fn collect_splits(&self, rect: Rect, out: &mut Vec<(SplitId, Axis, f32, Rect)>) {
        if let Self::Split {
            id,
            axis,
            ratio,
            a,
            b,
        } = self
        {
            out.push((*id, *axis, *ratio, rect));
            let (first, second) = split_rects(rect, *axis, *ratio);
            a.collect_splits(first, out);
            b.collect_splits(second, out);
        }
    }
}

fn split_rects(rect: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    match axis {
        Axis::Vertical => (
            Rect {
                w: rect.w * ratio,
                ..rect
            },
            Rect {
                x: rect.x + rect.w * ratio,
                w: rect.w * (1.0 - ratio),
                ..rect
            },
        ),
        Axis::Horizontal => (
            Rect {
                h: rect.h * ratio,
                ..rect
            },
            Rect {
                y: rect.y + rect.h * ratio,
                h: rect.h * (1.0 - ratio),
                ..rect
            },
        ),
    }
}

fn describe_node(node: &Node) -> LayoutNode {
    match node {
        Node::Leaf(id) => LayoutNode::Leaf(*id),
        Node::Split {
            id,
            axis,
            ratio,
            a,
            b,
        } => LayoutNode::Split {
            id: *id,
            axis: *axis,
            ratio: *ratio,
            a: Box::new(describe_node(a)),
            b: Box::new(describe_node(b)),
        },
    }
}

fn has_split(node: &Node, split_id: SplitId) -> bool {
    match node {
        Node::Leaf(_) => false,
        Node::Split { id, a, b, .. } => {
            *id == split_id || has_split(a, split_id) || has_split(b, split_id)
        }
    }
}

fn contains(node: &Node, pane: PaneId) -> bool {
    match node {
        Node::Leaf(id) => *id == pane,
        Node::Split { a, b, .. } => contains(a, pane) || contains(b, pane),
    }
}

fn deepest_split(node: &Node, pane: PaneId, found: Option<SplitId>) -> Option<SplitId> {
    match node {
        Node::Leaf(_) => found,
        Node::Split { id, a, b, .. } => {
            if contains(a, pane) {
                deepest_split(a, pane, Some(*id))
            } else if contains(b, pane) {
                deepest_split(b, pane, Some(*id))
            } else {
                None
            }
        }
    }
}

fn collect_panes(node: &Node, out: &mut Vec<PaneId>) {
    match node {
        Node::Leaf(id) => out.push(*id),
        Node::Split { a, b, .. } => {
            collect_panes(a, out);
            collect_panes(b, out);
        }
    }
}

fn collect_rects(node: &Node, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
    match node {
        Node::Leaf(id) => out.push((*id, rect)),
        Node::Split {
            axis, ratio, a, b, ..
        } => match axis {
            Axis::Vertical => {
                let left = Rect {
                    w: rect.w * ratio,
                    ..rect
                };
                let right = Rect {
                    x: rect.x + rect.w * ratio,
                    w: rect.w * (1.0 - ratio),
                    ..rect
                };
                collect_rects(a, left, out);
                collect_rects(b, right, out);
            }
            Axis::Horizontal => {
                // `y` grows downwards, matching the cell grid.
                let top = Rect {
                    h: rect.h * ratio,
                    ..rect
                };
                let bottom = Rect {
                    y: rect.y + rect.h * ratio,
                    h: rect.h * (1.0 - ratio),
                    ..rect
                };
                collect_rects(a, top, out);
                collect_rects(b, bottom, out);
            }
        },
    }
}

/// Returns the replacement for `node` (None when the subtree is gone) and
/// whether `pane` was found.
fn remove(node: Node, pane: PaneId) -> (Option<Node>, bool) {
    match node {
        Node::Leaf(id) if id == pane => (None, true),
        Node::Leaf(id) => (Some(Node::Leaf(id)), false),
        Node::Split {
            id,
            axis,
            ratio,
            a,
            b,
        } => {
            let (new_a, found) = remove(*a, pane);
            if found {
                return match new_a {
                    Some(a) => (
                        Some(Node::Split {
                            id,
                            axis,
                            ratio,
                            a: Box::new(a),
                            b,
                        }),
                        true,
                    ),
                    // The sibling takes the parent's place.
                    None => (Some(*b), true),
                };
            }
            let Some(a) = new_a else {
                return (Some(*b), false);
            };
            let (new_b, found) = remove(*b, pane);
            match new_b {
                Some(b) => (
                    Some(Node::Split {
                        id,
                        axis,
                        ratio,
                        a: Box::new(a),
                        b: Box::new(b),
                    }),
                    found,
                ),
                None => (Some(a), found),
            }
        }
    }
}

fn resize_in(
    node: &mut Node,
    split_id: SplitId,
    ratio: f32,
    cols: u16,
    rows: u16,
    min_cols: u16,
    min_rows: u16,
) -> Option<()> {
    match node {
        Node::Leaf(_) => None,
        Node::Split {
            id,
            axis,
            ratio: current,
            a,
            b,
        } => {
            if *id == split_id {
                *current = clamp_ratio(ratio, *axis, cols, rows, min_cols, min_rows);
                return Some(());
            }
            resize_in(a, split_id, ratio, cols, rows, min_cols, min_rows)
                .or_else(|| resize_in(b, split_id, ratio, cols, rows, min_cols, min_rows))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(1)
    }

    fn area(rects: &[(PaneId, Rect)]) -> f32 {
        rects.iter().map(|(_, r)| r.w * r.h).sum()
    }

    #[test]
    fn vertical_split_puts_panes_side_by_side() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        let rects = tree.rects();
        assert_eq!(rects.len(), 2);
        assert_eq!((rects[0].1.x, rects[0].1.w), (0.0, 0.5));
        assert_eq!((rects[1].1.x, rects[1].1.w), (0.5, 0.5));
        assert!((area(&rects) - 1.0).abs() < 1e-5, "rects tile the session");
    }

    #[test]
    fn horizontal_split_stacks_panes() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Horizontal, 0.25, 100, 40)
            .expect("split");
        let rects = tree.rects();
        assert_eq!((rects[0].1.y, rects[0].1.h), (0.0, 0.25));
        assert_eq!((rects[1].1.y, rects[1].1.h), (0.25, 0.75));
        assert!((area(&rects) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn nested_splits_make_three_and_four_panes() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Vertical, 0.5, 120, 40)
            .expect("split");
        tree.split(2, 3, Axis::Horizontal, 0.5, 120, 40)
            .expect("split");
        assert_eq!(tree.pane_ids(), vec![1, 2, 3]);
        let rects = tree.rects();
        assert_eq!(rects.len(), 3);
        assert!((area(&rects) - 1.0).abs() < 1e-5);

        tree.split(3, 4, Axis::Horizontal, 0.5, 120, 40)
            .expect("split");
        assert_eq!(tree.pane_ids(), vec![1, 2, 3, 4]);
        let rects = tree.rects();
        assert_eq!(rects.len(), 4);
        assert!((area(&rects) - 1.0).abs() < 1e-5);
        // No overlap between any two panes.
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let (a, b) = (rects[i].1, rects[j].1);
                let overlap = (a.x < b.x + b.w)
                    && (b.x < a.x + a.w)
                    && (a.y < b.y + b.h)
                    && (b.y < a.y + a.h);
                assert!(!overlap, "panes {i} and {j} overlap");
            }
        }
    }

    #[test]
    fn closing_a_pane_lets_the_sibling_fill_the_parent() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        let split_id = tree.rects()[0].1.w;
        assert!((split_id - 0.5).abs() < 1e-5);

        tree.close(2).expect("close");
        let rects = tree.rects();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, 1);
        assert_eq!(
            rects[0].1,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0
            }
        );
    }

    #[test]
    fn closing_the_only_pane_leaves_an_empty_tree() {
        let mut tree = layout();
        tree.close(1).expect("close");
        assert!(tree.is_empty());
        assert!(tree.rects().is_empty());
        assert_eq!(tree.focused(), None);
    }

    #[test]
    fn ratio_is_clamped_by_the_minimum_size() {
        let mut tree = Layout::new(1).with_minimums(10, 3);
        // 100 columns: a 10-column minimum allows 0.1..0.9.
        tree.split(1, 2, Axis::Vertical, 0.95, 100, 30)
            .expect("split");
        let rects = tree.rects();
        assert!((rects[0].1.w - 0.9).abs() < 1e-5, "clamped to 0.9");

        // 20 columns: the minimum is half the width, so the ratio sits at 0.5.
        let mut narrow = Layout::new(1).with_minimums(10, 3);
        narrow
            .split(1, 2, Axis::Vertical, 0.9, 20, 30)
            .expect("split");
        assert!((narrow.rects()[0].1.w - 0.5).abs() < 1e-5);
    }

    #[test]
    fn ratio_changes_clamp_too() {
        let mut tree = Layout::new(1).with_minimums(10, 3);
        let split = tree
            .split(1, 2, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        tree.set_ratio(split, 0.99, 100, 30).expect("resize");
        assert!((tree.rects()[0].1.w - 0.9).abs() < 1e-5);
        assert_eq!(
            tree.set_ratio(split + 99, 0.5, 100, 30),
            Err(PgError::NoSuchSplit)
        );
    }

    #[test]
    fn focus_cycles_in_tree_order_and_wraps() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        tree.split(2, 3, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        assert_eq!(tree.focused(), Some(1));
        assert_eq!(tree.focus_next(true), Some(2));
        assert_eq!(tree.focus_next(true), Some(3));
        assert_eq!(tree.focus_next(true), Some(1), "wraps forward");
        assert_eq!(tree.focus_next(false), Some(3), "wraps backward");
        assert!(tree.focus(2));
        assert_eq!(tree.focused(), Some(2));
        assert!(!tree.focus(99));
    }

    #[test]
    fn described_tree_reports_the_same_geometry() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Vertical, 0.3, 100, 30)
            .expect("split");
        tree.split(2, 3, Axis::Horizontal, 0.6, 100, 30)
            .expect("split");
        let described = tree.describe().expect("tree");
        assert_eq!(described.rects(), tree.rects());
        let splits = described.splits();
        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].1, Axis::Vertical);
        assert!((splits[0].2 - 0.3).abs() < 1e-5);
        assert_eq!(
            splits[0].3,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0
            }
        );
        assert_eq!(splits[1].1, Axis::Horizontal);
        assert!(
            (splits[1].3.x - 0.3).abs() < 1e-5,
            "inner split sits in the right half"
        );
    }

    #[test]
    fn split_of_finds_the_divider_next_to_a_pane() {
        let mut tree = layout();
        let outer = tree
            .split(1, 2, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        let inner = tree
            .split(2, 3, Axis::Horizontal, 0.5, 100, 30)
            .expect("split");
        assert_eq!(tree.split_of(1), Some(outer));
        assert_eq!(tree.split_of(2), Some(inner), "deepest split wins");
        assert_eq!(tree.split_of(3), Some(inner));
        assert_eq!(tree.split_of(99), None);
    }

    #[test]
    fn closing_the_focused_pane_moves_focus() {
        let mut tree = layout();
        tree.split(1, 2, Axis::Vertical, 0.5, 100, 30)
            .expect("split");
        tree.focus(2);
        tree.close(2).expect("close");
        assert_eq!(tree.focused(), Some(1));
    }

    #[test]
    fn splitting_a_missing_pane_is_a_no_op() {
        let mut tree = layout();
        assert_eq!(
            tree.split(99, 2, Axis::Vertical, 0.5, 100, 30),
            Err(PgError::NoSuchPane)
        );
        assert_eq!(tree.pane_ids(), vec![1], "the tree is untouched");
        assert_eq!(tree.close(99), Err(PgError::NoSuchPane));
        assert_eq!(
            tree.split(1, 1, Axis::Vertical, 0.5, 100, 30),
            Err(PgError::InvalidArg)
        );
    }
}
