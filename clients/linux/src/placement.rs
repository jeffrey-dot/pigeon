//! Maps host-provided pane rectangles to client pixels.

use pigeon_core::api::{PaneId, Rect};

/// Where each pane goes, in device pixels.
///
/// Split out of `Workspace` so the geometry is a pure function: the layout maths is the part
/// that is easy to get wrong, and it can be tested without a window.
pub fn place(
    layout: &[(PaneId, Rect)],
    zoom: Option<PaneId>,
    canvas: [f32; 2],
    known: &[PaneId],
) -> Vec<(PaneId, [f32; 2], [f32; 2])> {
    if let Some(zoom) = zoom.filter(|pane| known.contains(pane)) {
        return vec![(zoom, [0.0, 0.0], canvas)];
    }
    if layout.is_empty() {
        // No layout yet: the pane we attached to first owns the window.
        return known
            .iter()
            .min()
            .map(|pane| vec![(*pane, [0.0, 0.0], canvas)])
            .unwrap_or_default();
    }
    layout
        .iter()
        .filter(|(pane, _)| known.contains(pane))
        .map(|(pane, rect)| {
            (
                *pane,
                [rect.x * canvas[0], rect.y * canvas[1]],
                [rect.w * canvas[0], rect.h * canvas[1]],
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn side_by_side_panes_split_the_canvas() {
        let layout = [(1, rect(0.0, 0.0, 0.5, 1.0)), (2, rect(0.5, 0.0, 0.5, 1.0))];
        let placed = place(&layout, None, [1000.0, 400.0], &[1, 2]);
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0], (1, [0.0, 0.0], [500.0, 400.0]));
        assert_eq!(placed[1], (2, [500.0, 0.0], [500.0, 400.0]));
    }

    #[test]
    fn zoom_gives_one_pane_the_whole_area() {
        let layout = [(1, rect(0.0, 0.0, 0.5, 1.0)), (2, rect(0.5, 0.0, 0.5, 1.0))];
        let placed = place(&layout, Some(2), [800.0, 600.0], &[1, 2]);
        assert_eq!(placed, vec![(2, [0.0, 0.0], [800.0, 600.0])]);
    }

    #[test]
    fn a_pane_without_a_screen_yet_is_not_drawn() {
        // The layout names a pane whose snapshot has not arrived: drawing it would paint an
        // empty rect over its sibling's half.
        let layout = [(1, rect(0.0, 0.0, 0.5, 1.0)), (2, rect(0.5, 0.0, 0.5, 1.0))];
        let placed = place(&layout, None, [100.0, 100.0], &[2]);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].0, 2);
    }

    #[test]
    fn no_layout_yet_falls_back_to_the_first_pane() {
        let placed = place(&[], None, [10.0, 20.0], &[7]);
        assert_eq!(placed, vec![(7, [0.0, 0.0], [10.0, 20.0])]);
    }
}
