//! Exported Alacritty cells; text measurement and allocation use the same UTF-8 prefix.

use std::sync::Arc;

use alacritty_terminal::term::cell::{Cell as VtCell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::proto::MAX_CELL_TEXT_BYTES;
use crate::term::{
    Cell, ATTR_BOLD, ATTR_DIM, ATTR_ITALIC, ATTR_REVERSE, ATTR_STRIKETHROUGH, ATTR_UNDERLINE,
    ATTR_WRAP,
};

pub(super) struct ExportedText<'a> {
    base: char,
    marks: &'a [char],
    len: usize,
}

impl<'a> ExportedText<'a> {
    pub(super) fn new(cell: &'a VtCell) -> Self {
        // Alacritty retains HT in the first cell of a tab stop; clients draw a space.
        let base = if cell.c < ' ' { ' ' } else { cell.c };
        let marks = cell.zerowidth().unwrap_or_default();
        let mut len = base.len_utf8();
        let count = marks
            .iter()
            .take_while(|mark| {
                let next = len + mark.len_utf8();
                if next > MAX_CELL_TEXT_BYTES {
                    return false;
                }
                len = next;
                true
            })
            .count();
        Self {
            base,
            marks: &marks[..count],
            len,
        }
    }

    pub(super) fn extra_len(&self) -> Option<usize> {
        (!self.marks.is_empty()).then_some(self.len)
    }

    fn extra_text(&self) -> Option<Arc<str>> {
        let mut text = String::with_capacity(self.extra_len()?);
        text.push(self.base);
        text.extend(self.marks.iter().copied());
        Some(Arc::from(text))
    }
}

pub(super) fn convert(cell: &VtCell, colors: &Colors) -> Cell {
    let (attrs, width) = attributes(cell.flags);
    let text = ExportedText::new(cell);
    Cell {
        codepoint: text.base as u32,
        fg: color_to_u32(cell.fg, colors),
        bg: color_to_u32(cell.bg, colors),
        attrs,
        // OSC 8 needs a URI table the protocol does not yet carry.
        link: 0,
        width,
        extra_text: text.extra_text(),
    }
}

fn attributes(flags: Flags) -> (u32, u8) {
    let mut attrs = 0;
    if flags.contains(Flags::BOLD) {
        attrs |= ATTR_BOLD;
    }
    if flags.contains(Flags::ITALIC) {
        attrs |= ATTR_ITALIC;
    }
    if flags.intersects(Flags::ALL_UNDERLINES) {
        attrs |= ATTR_UNDERLINE;
    }
    if flags.contains(Flags::INVERSE) {
        attrs |= ATTR_REVERSE;
    }
    if flags.contains(Flags::STRIKEOUT) {
        attrs |= ATTR_STRIKETHROUGH;
    }
    if flags.contains(Flags::DIM) {
        attrs |= ATTR_DIM;
    }
    if flags.contains(Flags::WRAPLINE) {
        attrs |= ATTR_WRAP;
    }
    let width = if flags.contains(Flags::WIDE_CHAR) {
        2
    } else if flags.contains(Flags::WIDE_CHAR_SPACER) {
        0
    } else {
        1
    };
    (attrs, width)
}

fn rgb_to_u32(rgb: Rgb) -> u32 {
    (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
}

fn color_to_u32(color: Color, colors: &Colors) -> u32 {
    match color {
        Color::Spec(rgb) => rgb_to_u32(rgb),
        Color::Indexed(index) => 0x0100_0000 | u32::from(index),
        // Named colours follow the theme; resolve them to RGB for the snapshot.
        // `Colors` only stores overrides, so fall back to xterm's defaults.
        Color::Named(named) => rgb_to_u32(colors[named].unwrap_or_else(|| default_rgb(named))),
    }
}

/// xterm's defaults for the colours a theme may not override.
fn default_rgb(named: NamedColor) -> Rgb {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let index = named as usize;
    match index {
        0..=15 => {
            let (r, g, b) = ANSI[index];
            Rgb { r, g, b }
        }
        // Dim variants follow Cursor (258): 259..=266 are the ANSI colours at 2/3.
        259..=266 => {
            let (r, g, b) = ANSI[index - 259];
            Rgb {
                r: r * 2 / 3,
                g: g * 2 / 3,
                b: b * 2 / 3,
            }
        }
        // Background and its dim variant are black, not light grey.
        257 | 268 => Rgb { r: 0, g: 0, b: 0 },
        // Foreground, Cursor and BrightForeground.
        _ => Rgb {
            r: 229,
            g: 229,
            b: 229,
        },
    }
}
