//! Pixel hit testing and pointer-to-ratio mapping; the core owns layout and minimum sizes.
use pigeon_core::api::{Axis, Split};

pub fn line(split: &Split, canvas: [f32; 2], thickness: f32) -> [f32; 4] {
    let r = split.rect;
    match split.axis {
        Axis::Vertical => [
            (r.x + r.w * split.ratio) * canvas[0] - thickness / 2.0,
            r.y * canvas[1],
            thickness,
            r.h * canvas[1],
        ],
        Axis::Horizontal => [
            r.x * canvas[0],
            (r.y + r.h * split.ratio) * canvas[1] - thickness / 2.0,
            r.w * canvas[0],
            thickness,
        ],
    }
}

pub fn hit(splits: &[Split], zoom: bool, canvas: [f32; 2], point: [f32; 2]) -> Option<Split> {
    if zoom
        || !canvas.iter().all(|v| v.is_finite() && *v > 0.0)
        || !point.iter().all(|v| v.is_finite())
    {
        return None;
    }
    // Nested dividers are later in tree order; prefer their narrower hit region at a junction.
    splits.iter().rev().copied().find(|split| {
        let [x, y, w, h] = line(split, canvas, 1.0);
        match split.axis {
            Axis::Vertical => (point[0] - x).abs() <= 5.0 && point[1] >= y && point[1] <= y + h,
            Axis::Horizontal => (point[1] - y).abs() <= 5.0 && point[0] >= x && point[0] <= x + w,
        }
    })
}

pub fn ratio(split: &Split, canvas: [f32; 2], point: [f32; 2]) -> Option<f32> {
    let (coordinate, origin, length) = match split.axis {
        Axis::Vertical => (point[0], split.rect.x * canvas[0], split.rect.w * canvas[0]),
        Axis::Horizontal => (point[1], split.rect.y * canvas[1], split.rect.h * canvas[1]),
    };
    if !coordinate.is_finite() || !origin.is_finite() || !length.is_finite() || length <= 0.0 {
        return None;
    }
    Some(((coordinate - origin) / length).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::api::Rect;
    fn split(axis: Axis) -> Split {
        Split {
            id: 2,
            axis,
            ratio: 0.25,
            rect: Rect {
                x: 0.5,
                y: 0.0,
                w: 0.5,
                h: 1.0,
            },
        }
    }
    #[test]
    fn divider_line_and_drag_use_the_host_split_box_and_ratio() {
        let vertical = split(Axis::Vertical);
        assert_eq!(
            line(&vertical, [800.0, 400.0], 2.0),
            [499.0, 0.0, 2.0, 400.0]
        );
        assert_eq!(
            hit(&[vertical], false, [800.0, 400.0], [502.0, 200.0]),
            Some(vertical)
        );
        assert_eq!(ratio(&vertical, [800.0, 400.0], [600.0, 200.0]), Some(0.5));
        let horizontal = split(Axis::Horizontal);
        assert_eq!(
            line(&horizontal, [800.0, 400.0], 2.0),
            [400.0, 99.0, 400.0, 2.0]
        );
        assert_eq!(
            ratio(&horizontal, [800.0, 400.0], [500.0, 300.0]),
            Some(0.75)
        );
    }
    #[test]
    fn zoom_and_outside_points_never_start_a_divider_gesture() {
        let split = split(Axis::Vertical);
        assert_eq!(hit(&[split], true, [800.0, 400.0], [500.0, 200.0]), None);
        assert_eq!(hit(&[split], false, [800.0, 400.0], [500.0, 500.0]), None);
        assert_eq!(hit(&[split], false, [800.0, 400.0], [f32::NAN, 0.0]), None);
        assert_eq!(ratio(&split, [0.0, 400.0], [20.0, 30.0]), None);
        assert_eq!(ratio(&split, [800.0, 400.0], [1000.0, 30.0]), Some(1.0));
    }
}
