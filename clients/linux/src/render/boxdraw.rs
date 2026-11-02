//! Box-drawing (U+2500–257F) and block elements (U+2580–2593) drawn as rectangles.
//!
//! A font's `─` rarely spans the full cell at every size and line height, so TUI borders
//! break into dashes; rectangles computed from the cell always meet. Port of
//! `clients/macos/Sources/PigeonApp/Terminal/Render/BoxDrawing.swift` — same table, same
//! geometry, so the two clients draw `┌─┐` identically.

use super::instances::RectInstance;

/// Stroke weight of one arm, counted from the cell centre.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Weight {
    None = 0,
    Light = 1,
    Heavy = 2,
    Double = 3,
}

fn weight(value: u8) -> Weight {
    match value {
        1 => Weight::Light,
        2 => Weight::Heavy,
        3 => Weight::Double,
        _ => Weight::None,
    }
}

/// `(left, right, up, down)` as weight values; see the Swift file for the same table.
fn arms(codepoint: u32) -> Option<[u8; 4]> {
    Some(match codepoint {
        0x2500 => [1, 1, 0, 0],
        0x2501 => [2, 2, 0, 0],
        0x2502 => [0, 0, 1, 1],
        0x2503 => [0, 0, 2, 2],
        0x250C => [0, 1, 0, 1],
        0x250D => [0, 2, 0, 1],
        0x250E => [0, 1, 0, 2],
        0x250F => [0, 2, 0, 2],
        0x2510 => [1, 0, 0, 1],
        0x2511 => [2, 0, 0, 1],
        0x2512 => [1, 0, 0, 2],
        0x2513 => [2, 0, 0, 2],
        0x2514 => [0, 1, 1, 0],
        0x2515 => [0, 2, 1, 0],
        0x2516 => [0, 1, 2, 0],
        0x2517 => [0, 2, 2, 0],
        0x2518 => [1, 0, 1, 0],
        0x2519 => [2, 0, 1, 0],
        0x251A => [1, 0, 2, 0],
        0x251B => [2, 0, 2, 0],
        0x251C => [0, 1, 1, 1],
        0x2523 => [0, 2, 2, 2],
        0x2524 => [1, 0, 1, 1],
        0x252B => [2, 0, 2, 2],
        0x252C => [1, 1, 0, 1],
        0x2533 => [2, 2, 0, 2],
        0x2534 => [1, 1, 1, 0],
        0x253B => [2, 2, 2, 0],
        0x253C => [1, 1, 1, 1],
        0x254B => [2, 2, 2, 2],
        0x2550 => [3, 3, 0, 0],
        0x2551 => [0, 0, 3, 3],
        0x2554 => [0, 3, 0, 3],
        0x2557 => [3, 0, 0, 3],
        0x255A => [0, 3, 3, 0],
        0x255D => [3, 0, 3, 0],
        0x2560 => [0, 3, 3, 3],
        0x2563 => [3, 0, 3, 3],
        0x2566 => [3, 3, 0, 3],
        0x2569 => [3, 3, 3, 0],
        0x256C => [3, 3, 3, 3],
        0x2552 => [0, 3, 0, 1],
        0x2553 => [0, 1, 0, 3],
        0x2555 => [3, 0, 0, 1],
        0x2556 => [1, 0, 0, 3],
        0x2558 => [0, 3, 1, 0],
        0x2559 => [0, 1, 3, 0],
        0x255B => [3, 0, 1, 0],
        0x255C => [1, 0, 3, 0],
        0x255E => [0, 3, 1, 1],
        0x255F => [0, 1, 3, 3],
        0x2561 => [3, 0, 1, 1],
        0x2562 => [1, 0, 3, 3],
        0x2564 => [3, 3, 0, 1],
        0x2565 => [1, 1, 0, 3],
        0x2567 => [3, 3, 1, 0],
        0x2568 => [1, 1, 3, 0],
        0x256A => [3, 3, 1, 1],
        0x256B => [1, 1, 3, 3],
        0x256D => [0, 1, 0, 1],
        0x256E => [1, 0, 0, 1],
        0x256F => [1, 0, 1, 0],
        0x2570 => [0, 1, 1, 0],
        0x2574 => [1, 0, 0, 0],
        0x2575 => [0, 0, 1, 0],
        0x2576 => [0, 1, 0, 0],
        0x2577 => [0, 0, 0, 1],
        0x2578 => [2, 0, 0, 0],
        0x2579 => [0, 0, 2, 0],
        0x257A => [0, 2, 0, 0],
        0x257B => [0, 0, 0, 2],
        _ => return None,
    })
}

/// Whether `codepoint` is drawn here rather than looked up in the font.
pub fn handles(codepoint: u32) -> bool {
    arms(codepoint).is_some() || (0x2580..=0x2593).contains(&codepoint)
}

/// The rectangles for `codepoint` in the cell at `origin` of `size`; `false` means the font
/// should draw it. `thin` is the light stroke in device pixels.
pub fn draw(
    codepoint: u32,
    origin: [f32; 2],
    size: [f32; 2],
    thin: f32,
    color: [f32; 4],
    out: &mut Vec<RectInstance>,
) -> bool {
    if let Some(block) = block(codepoint, origin, size, color) {
        out.push(block);
        return true;
    }
    let Some([left, right, up, down]) = arms(codepoint) else {
        return false;
    };
    let light = thin.round().max(1.0);
    let centre = [
        ((origin[0] + size[0] / 2.0 - light / 2.0) as i32) as f32,
        ((origin[1] + size[1] / 2.0 - light / 2.0) as i32) as f32,
    ];
    // Each arm runs from the cell edge to just past the centre, so arms meet and a
    // neighbouring cell's arm continues the line without a gap.
    let mut horizontal = |weight: Weight, to_left: bool| {
        let (width, offsets) = stroke(weight, light);
        for offset in offsets {
            let y = ((centre[1] + offset) as i32) as f32;
            let start = if to_left { origin[0] } else { centre[0] };
            let end = if to_left {
                centre[0] + width
            } else {
                origin[0] + size[0]
            };
            out.push(RectInstance {
                pos: [start, y],
                size: [end - start, width],
                color,
            });
        }
    };
    horizontal(weight(left), true);
    horizontal(weight(right), false);
    let mut vertical = |weight: Weight, to_top: bool| {
        let (width, offsets) = stroke(weight, light);
        for offset in offsets {
            let x = ((centre[0] + offset) as i32) as f32;
            let start = if to_top { origin[1] } else { centre[1] };
            let end = if to_top {
                centre[1] + width
            } else {
                origin[1] + size[1]
            };
            out.push(RectInstance {
                pos: [x, start],
                size: [width, end - start],
                color,
            });
        }
    };
    vertical(weight(up), true);
    vertical(weight(down), false);
    true
}

/// `(width, offsets from the centre)` for one arm at this weight.
fn stroke(weight: Weight, light: f32) -> (f32, Vec<f32>) {
    match weight {
        Weight::None => (0.0, Vec::new()),
        Weight::Light => (light, vec![0.0]),
        Weight::Heavy => (light * 2.0, vec![-light / 2.0]),
        Weight::Double => (light, vec![-light, light]),
    }
}

/// Halves, eighths, the full block and the three shades.
fn block(
    codepoint: u32,
    origin: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
) -> Option<RectInstance> {
    let rect = |x: f32, y: f32, width: f32, height: f32, alpha: f32| {
        let left = ((origin[0] + x * size[0]) as i32) as f32;
        let top = ((origin[1] + y * size[1]) as i32) as f32;
        let right = ((origin[0] + (x + width) * size[0]) as i32) as f32;
        let bottom = ((origin[1] + (y + height) * size[1]) as i32) as f32;
        let mut tinted = color;
        tinted[3] *= alpha;
        RectInstance {
            pos: [left, top],
            size: [right - left, bottom - top],
            color: tinted,
        }
    };
    const FULL: f32 = 1.0;
    Some(match codepoint {
        0x2580 => rect(0.0, 0.0, FULL, 0.5, 1.0),
        0x2581..=0x2588 => {
            let eighths = (codepoint - 0x2580) as f32 / 8.0;
            rect(0.0, 1.0 - eighths, FULL, eighths, 1.0)
        }
        0x2589..=0x258F => {
            let eighths = (0x2590 - codepoint) as f32 / 8.0;
            rect(0.0, 0.0, eighths, FULL, 1.0)
        }
        0x2590 => rect(0.5, 0.0, 0.5, FULL, 1.0),
        0x2591 => rect(0.0, 0.0, FULL, FULL, 0.25),
        0x2592 => rect(0.0, 0.0, FULL, FULL, 0.5),
        0x2593 => rect(0.0, 0.0, FULL, FULL, 0.75),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> ([f32; 2], [f32; 2]) {
        ([0.0, 0.0], [8.0, 16.0])
    }

    #[test]
    fn a_horizontal_rule_spans_the_whole_cell() {
        let (origin, size) = cell();
        let mut out = Vec::new();
        assert!(draw(0x2500, origin, size, 1.0, [1.0; 4], &mut out));
        // Two arms, one from each edge to the centre: together they cover the cell, so the
        // neighbouring cell's arm continues the line.
        assert_eq!(out.len(), 2);
        let left = out[0].pos[0].min(out[1].pos[0]);
        let right_edge = out
            .iter()
            .map(|rect| rect.pos[0] + rect.size[0])
            .fold(0.0f32, f32::max);
        assert!((left - origin[0]).abs() < 0.001);
        assert!((right_edge - (origin[0] + size[0])).abs() < 0.001);
        assert!(out.iter().all(|rect| rect.size[1] == 1.0));
    }

    #[test]
    fn a_corner_has_two_arms_that_meet() {
        let (origin, size) = cell();
        let mut out = Vec::new();
        assert!(draw(0x250C, origin, size, 1.0, [1.0; 4], &mut out));
        assert_eq!(out.len(), 2, "┌ is a right arm plus a down arm");
        // The right arm starts at the centre and reaches the cell edge.
        let right = out
            .iter()
            .find(|r| r.size[0] > r.size[1])
            .expect("horizontal arm");
        assert!(right.pos[0] <= size[0] / 2.0);
        assert!((right.pos[0] + right.size[0] - size[0]).abs() < 0.001);
    }

    #[test]
    fn blocks_are_fractions_of_the_cell() {
        let (origin, size) = cell();
        let mut out = Vec::new();
        assert!(draw(0x2588, origin, size, 1.0, [1.0; 4], &mut out));
        assert_eq!(out[0].size, [size[0], size[1]], "█ fills the cell");

        out.clear();
        assert!(draw(0x2592, origin, size, 1.0, [1.0; 4], &mut out));
        assert!((out[0].color[3] - 0.5).abs() < 0.001, "▒ is half opacity");

        out.clear();
        assert!(
            !draw(0x41, origin, size, 1.0, [1.0; 4], &mut out),
            "'A' is the font's job"
        );
        assert!(!handles(0x41));
    }

    #[test]
    fn every_weight_of_cross_has_four_arms() {
        let (origin, size) = cell();
        let mut out = Vec::new();
        assert!(draw(0x253C, origin, size, 1.0, [1.0; 4], &mut out));
        assert_eq!(out.len(), 4, "┼ is four arms");
        out.clear();
        assert!(draw(0x256C, origin, size, 1.0, [1.0; 4], &mut out));
        assert_eq!(out.len(), 8, "╬ is four double arms");
    }
}
