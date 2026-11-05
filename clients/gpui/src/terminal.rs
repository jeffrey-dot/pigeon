//! The terminal view: turns a pane's `Screen` into GPUI elements.
//!
//! The VT semantics, snapshot and delta all come from `core/` through `pigeon_core::api`; this
//! module only paints. Cell colours resolve through `palette` (shared logic with the other
//! clients), and the screen model lives in `screen`.
//!
//! Stage: render the live grid as per-row runs of coloured monospace text. A custom GPUI
//! `Element` with glyph batching (and cursor/selection/scrollback) is a later refinement; this
//! already shows real program output, which is the bulk of the reference screenshot.

pub mod palette;
pub mod screen;

use gpui::{div, px, rgb, Div, FontWeight, ParentElement, Styled};
use pigeon_core::api::{ATTR_BOLD, ATTR_ITALIC};

pub use palette::Palette;
pub use screen::Screen;

use crate::theme::fonts::{TERMINAL_LINE_HEIGHT, TERMINAL_SIZE};

/// Renders the live screen of `screen` into a scrollable grid element, resolving colours with
/// `palette`. One child per row; each row is a flex of coloured text spans coalesced by style.
pub fn grid(screen: &Screen, palette: &Palette) -> Div {
    let snapshot = &screen.snapshot;
    let mut column = div()
        .flex()
        .flex_col()
        .text_size(px(TERMINAL_SIZE))
        .line_height(px(TERMINAL_LINE_HEIGHT))
        .whitespace_nowrap();

    for row in &snapshot.screen {
        let mut line = div().flex().h(px(TERMINAL_LINE_HEIGHT)).items_center();
        // Coalesce adjacent cells that share a style into one span, so a line of plain text is
        // a single text run rather than one element per cell.
        let mut run = String::new();
        let mut run_style: Option<(u32, u32, u32)> = None;

        let flush = |line: Div, run: &mut String, style: Option<(u32, u32, u32)>| -> Div {
            if run.is_empty() {
                return line;
            }
            let (fg, bg, attrs) = style.unwrap_or((0, 0, 0));
            let (front, back) = palette.colors(fg, bg, attrs);
            let mut span = div()
                .text_color(rgb(front))
                .bg(rgb(back))
                .child(std::mem::take(run));
            if attrs & ATTR_BOLD != 0 {
                span = span.font_weight(FontWeight::BOLD);
            }
            if attrs & ATTR_ITALIC != 0 {
                span = span.italic();
            }
            line.child(span)
        };

        for cell in row {
            // Wide-char continuation cells carry no text; skip them (the owner drew the glyph).
            if cell.width == 0 {
                continue;
            }
            let style = (cell.fg, cell.bg, cell.attrs);
            if run_style != Some(style) {
                line = flush(line, &mut run, run_style);
                run_style = Some(style);
            }
            run.push_str(&cell.text());
        }
        line = flush(line, &mut run, run_style);
        column = column.child(line);
    }
    column
}
