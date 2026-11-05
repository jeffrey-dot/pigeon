//! Sidebar glyphs drawn with GPUI paths: the row status icons (`docs/ui.md` §3.3) and the
//! top-bar symbols (§2). Sizes and colour roles follow Harp's sidebar (`status_icon`: a 15 px
//! glyph centred in a 16 px slot); the shapes are our own geometry, not Harp's assets.

use gpui::{
    canvas, point, px, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Rgba, Styled, Window,
};
use pigeon_core::api::StatusState as State;

use crate::theme::{metrics, Palette};

/// What the trailing status slot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusGlyph {
    Working,
    Needs,
    DoneUnread,
    DoneRead,
    Idle,
    Error,
}

impl StatusGlyph {
    /// The glyph for a session's aggregate state; `None` (no OSC 7501 report) draws nothing.
    pub fn for_state(state: Option<State>, unread: bool) -> Option<Self> {
        Some(match state? {
            State::Working => Self::Working,
            State::Blocked => Self::Needs,
            State::Done if unread => Self::DoneUnread,
            State::Done => Self::DoneRead,
            State::Idle => Self::Idle,
            State::Error => Self::Error,
        })
    }
}

/// Error red: system red per appearance (`docs/ui.md` §3.3).
fn error_red(dark: bool) -> Rgba {
    if dark {
        gpui::rgb(0xff453a)
    } else {
        gpui::rgb(0xff3b30)
    }
}

fn hsla(color: Rgba) -> Hsla {
    color.into()
}

/// A 16×16 slot with the glyph painted in its centre.
pub fn status(glyph: Option<StatusGlyph>, palette: &Palette, dark: bool) -> impl IntoElement {
    let palette = *palette;
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let Some(glyph) = glyph else { return };
            paint_status(glyph, bounds, &palette, dark, window);
        },
    )
    .w(px(metrics::STATUS_SLOT))
    .h(px(metrics::STATUS_SLOT))
    .flex_shrink_0()
}

fn circle(builder: &mut PathBuilder, center: Point<Pixels>, radius: f32) {
    let r = px(radius);
    builder.move_to(point(center.x, center.y - r));
    builder.arc_to(
        point(r, r),
        px(0.),
        false,
        true,
        point(center.x, center.y + r),
    );
    builder.arc_to(
        point(r, r),
        px(0.),
        false,
        true,
        point(center.x, center.y - r),
    );
    builder.close();
}

/// A filled crescent: the outer disc minus a disc shifted up and right. The two arcs meet at the
/// circles' intersection points.
fn crescent(builder: &mut PathBuilder, center: Point<Pixels>) {
    let (big, small) = (5.6_f32, 4.6_f32);
    let (dx, dy) = (2.8_f32, -2.0_f32);
    let [first, second] = circle_intersections(big, (dx, dy), small);
    let at = |(x, y): (f32, f32)| point(center.x + px(x), center.y + px(y));
    builder.move_to(at(first));
    // Round the outer disc the long way (through the lower left), then back along the inner one.
    builder.arc_to(point(px(big), px(big)), px(0.), true, false, at(second));
    builder.arc_to(point(px(small), px(small)), px(0.), false, true, at(first));
    builder.close();
}

/// Intersections of a circle of radius `r0` at the origin with one of radius `r1` at `offset`.
fn circle_intersections(r0: f32, offset: (f32, f32), r1: f32) -> [(f32, f32); 2] {
    let (x, y) = offset;
    let d = (x * x + y * y).sqrt();
    let a = (r0 * r0 - r1 * r1 + d * d) / (2. * d);
    let h = (r0 * r0 - a * a).max(0.).sqrt();
    let (mx, my) = (a * x / d, a * y / d);
    [
        (mx + h * y / d, my - h * x / d),
        (mx - h * y / d, my + h * x / d),
    ]
}

fn paint(builder: PathBuilder, color: Rgba, window: &mut Window) {
    if let Ok(path) = builder.build() {
        window.paint_path(path, hsla(color));
    }
}

fn paint_status(
    glyph: StatusGlyph,
    bounds: Bounds<Pixels>,
    palette: &Palette,
    dark: bool,
    window: &mut Window,
) {
    let size = metrics::STATUS_GLYPH;
    let center = bounds.center();
    // Ring radius leaves room for the stroke inside the 15 px glyph box.
    let stroke = 1.6;
    let radius = size / 2. - stroke / 2. - 0.5;
    let at = |dx: f32, dy: f32| point(center.x + px(dx), center.y + px(dy));

    match glyph {
        StatusGlyph::Working => {
            // Full grey track, then a strong three-quarter arc leaving the top-right quarter
            // open, as in the reference: clockwise from 3 o'clock round to 12 o'clock.
            let mut track = PathBuilder::stroke(px(stroke));
            circle(&mut track, center, radius);
            paint(track, palette.track, window);
            let mut arc = PathBuilder::stroke(px(stroke));
            arc.move_to(at(radius, 0.));
            arc.arc_to(
                point(px(radius), px(radius)),
                px(0.),
                true,
                true,
                at(0., -radius),
            );
            paint(arc, palette.icon_strong, window);
        }
        StatusGlyph::Needs => {
            let mut ring = PathBuilder::stroke(px(stroke));
            circle(&mut ring, center, radius);
            paint(ring, palette.icon_needs_ring, window);
            let mut dot = PathBuilder::fill();
            circle(&mut dot, center, radius * 0.42);
            paint(dot, palette.icon_needs_dot, window);
        }
        StatusGlyph::DoneUnread => {
            let mut disc = PathBuilder::fill();
            circle(&mut disc, center, size / 2. - 0.5);
            paint(disc, palette.icon_strong, window);
            let mut tick = PathBuilder::stroke(px(1.7));
            tick.move_to(at(-3.4, 0.2));
            tick.line_to(at(-0.9, 2.7));
            tick.line_to(at(3.6, -2.4));
            paint(tick, palette.icon_on_fill, window);
        }
        StatusGlyph::DoneRead => {
            let mut tick = PathBuilder::stroke(px(1.2));
            tick.move_to(at(-4.2, 0.3));
            tick.line_to(at(-1.2, 3.2));
            tick.line_to(at(4.3, -3.0));
            paint(tick, palette.ring, window);
        }
        StatusGlyph::Idle => {
            let mut moon = PathBuilder::fill();
            crescent(&mut moon, center);
            paint(moon, palette.moon, window);
        }
        StatusGlyph::Error => {
            let red = error_red(dark);
            let mut triangle = PathBuilder::stroke(px(1.4));
            triangle.add_polygon(&[at(0., -6.), at(6.4, 5.2), at(-6.4, 5.2)], true);
            paint(triangle, red, window);
            let mut bar = PathBuilder::stroke(px(1.4));
            bar.move_to(at(0., -2.2));
            bar.line_to(at(0., 1.4));
            paint(bar, red, window);
            let mut dot = PathBuilder::fill();
            circle(&mut dot, at(0., 3.4), 0.8);
            paint(dot, red, window);
        }
    }
}

/// Top-bar symbols (`docs/ui.md` §2: 17 px symbols in 28 px buttons).
#[derive(Clone, Copy)]
pub enum TopSymbol {
    Sidebar,
    Plus,
}

pub fn top_symbol(symbol: TopSymbol, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let center = bounds.center();
            let at = |dx: f32, dy: f32| point(center.x + px(dx), center.y + px(dy));
            match symbol {
                TopSymbol::Sidebar => {
                    // Rounded rectangle with a divider a third from the left.
                    let (w, h, r) = (8.0_f32, 6.5_f32, 2.0_f32);
                    let mut frame = PathBuilder::stroke(px(1.3));
                    frame.move_to(at(-w + r, -h));
                    frame.line_to(at(w - r, -h));
                    frame.arc_to(point(px(r), px(r)), px(0.), false, true, at(w, -h + r));
                    frame.line_to(at(w, h - r));
                    frame.arc_to(point(px(r), px(r)), px(0.), false, true, at(w - r, h));
                    frame.line_to(at(-w + r, h));
                    frame.arc_to(point(px(r), px(r)), px(0.), false, true, at(-w, h - r));
                    frame.line_to(at(-w, -h + r));
                    frame.arc_to(point(px(r), px(r)), px(0.), false, true, at(-w + r, -h));
                    frame.close();
                    paint(frame, color, window);
                    let mut divider = PathBuilder::stroke(px(1.3));
                    divider.move_to(at(-2.6, -h));
                    divider.line_to(at(-2.6, h));
                    paint(divider, color, window);
                }
                TopSymbol::Plus => {
                    let arm = 6.5;
                    let mut plus = PathBuilder::stroke(px(1.4));
                    plus.move_to(at(-arm, 0.));
                    plus.line_to(at(arm, 0.));
                    plus.move_to(at(0., -arm));
                    plus.line_to(at(0., arm));
                    paint(plus, color, window);
                }
            }
        },
    )
    .w(px(metrics::TOP_BUTTON_GLYPH))
    .h(px(metrics::TOP_BUTTON_GLYPH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crescent_endpoints_lie_on_both_circles() {
        for (x, y) in circle_intersections(5.6, (2.8, -2.0), 4.6) {
            assert!(((x * x + y * y).sqrt() - 5.6).abs() < 1e-3);
            assert!((((x - 2.8).powi(2) + (y + 2.0).powi(2)).sqrt() - 4.6).abs() < 1e-3);
        }
    }

    #[test]
    fn glyph_follows_state_and_unread() {
        assert_eq!(StatusGlyph::for_state(None, false), None);
        assert_eq!(
            StatusGlyph::for_state(Some(State::Working), false),
            Some(StatusGlyph::Working)
        );
        assert_eq!(
            StatusGlyph::for_state(Some(State::Blocked), false),
            Some(StatusGlyph::Needs)
        );
        assert_eq!(
            StatusGlyph::for_state(Some(State::Done), true),
            Some(StatusGlyph::DoneUnread)
        );
        assert_eq!(
            StatusGlyph::for_state(Some(State::Done), false),
            Some(StatusGlyph::DoneRead)
        );
        assert_eq!(
            StatusGlyph::for_state(Some(State::Idle), false),
            Some(StatusGlyph::Idle)
        );
        assert_eq!(
            StatusGlyph::for_state(Some(State::Error), false),
            Some(StatusGlyph::Error)
        );
    }
}
