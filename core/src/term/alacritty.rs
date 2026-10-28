//! `alacritty_terminal` behind the [`Term`](super::Term) trait.
//!
//! Nothing above this file knows about alacritty's types: snapshots are converted
//! to our own cells here, so swapping engines never reaches the ABI or protocol.

use std::sync::{Arc, Mutex};

use crate::term::Palette;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term as VtTerm, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb};

use super::{
    Cell, Cursor, Snapshot, Term, ATTR_BOLD, ATTR_DIM, ATTR_ITALIC, ATTR_REVERSE,
    ATTR_STRIKETHROUGH, ATTR_UNDERLINE,
};

/// Grid geometry handed to alacritty. `history` is the scrollback capacity.
#[derive(Clone, Copy)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub history: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize + self.history
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// Collects the bytes the terminal owes the application (DA/DSR/size answers).
#[derive(Clone)]
struct ReplySink {
    out: Arc<Mutex<Vec<u8>>>,
    size: Arc<Mutex<GridSize>>,
    title: Arc<Mutex<Option<String>>>,
    /// The client's colours; without them `OSC 4/10/11` stay unanswered.
    palette: Arc<Mutex<Option<Palette>>>,
}

impl EventListener for ReplySink {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                if let Ok(mut out) = self.out.lock() {
                    out.extend_from_slice(text.as_bytes());
                }
            }
            Event::TextAreaSizeRequest(format) => {
                let size = self.size.lock().map(|size| *size).unwrap_or(GridSize {
                    cols: 80,
                    rows: 24,
                    history: 0,
                });
                let window = WindowSize {
                    num_lines: size.rows,
                    num_cols: size.cols,
                    cell_width: 8,
                    cell_height: 16,
                };
                let reply = format(window);
                if let Ok(mut out) = self.out.lock() {
                    out.extend_from_slice(reply.as_bytes());
                }
            }
            // The program's title (OSC 0/2) is how the sidebar names the app.
            Event::Title(text) => {
                if let Ok(mut slot) = self.title.lock() {
                    *slot = Some(text);
                }
            }
            // The application asks what colour it is drawing on (`OSC 4/10/11`), typically at
            // startup, and picks its light or dark theme from the answer.
            Event::ColorRequest(index, format) => {
                let palette = self.palette.lock().ok().and_then(|slot| *slot);
                let Some(colour) = palette.and_then(|palette| palette.resolve(index)) else {
                    return;
                };
                let reply = format(Rgb {
                    r: colour.r,
                    g: colour.g,
                    b: colour.b,
                });
                if let Ok(mut out) = self.out.lock() {
                    out.extend_from_slice(reply.as_bytes());
                }
            }
            // OSC 52 clipboard access stays unanswered (see docs/plan.md §11).
            _ => {}
        }
    }
}

/// One pane's terminal state.
pub struct AlacrittyTerm {
    term: VtTerm<ReplySink>,
    parser: Processor,
    replies: Arc<Mutex<Vec<u8>>>,
    size: Arc<Mutex<GridSize>>,
    title: Arc<Mutex<Option<String>>>,
    palette: Arc<Mutex<Option<Palette>>>,
}

impl AlacrittyTerm {
    pub fn new(cols: u16, rows: u16, history: usize) -> Self {
        let size = GridSize {
            cols,
            rows,
            history,
        };
        let replies = Arc::new(Mutex::new(Vec::new()));
        let size_cell = Arc::new(Mutex::new(size));
        let title = Arc::new(Mutex::new(None));
        let palette = Arc::new(Mutex::new(None));
        let sink = ReplySink {
            out: Arc::clone(&replies),
            size: Arc::clone(&size_cell),
            title: Arc::clone(&title),
            palette: Arc::clone(&palette),
        };
        let config = Config {
            scrolling_history: history,
            ..Config::default()
        };
        Self {
            term: VtTerm::new(config, &size, sink),
            parser: Processor::new(),
            replies,
            size: size_cell,
            title,
            palette,
        }
    }

    fn convert_row(&self, line: Line, colors: &Colors) -> Vec<Cell> {
        let grid = self.term.grid();
        let cols = self
            .size
            .lock()
            .map(|size| size.cols as usize)
            .unwrap_or(80);
        (0..cols)
            .map(|col| {
                let cell = &grid[Point::new(line, Column(col))];
                let (attrs, width) = attributes(cell.flags);
                Cell {
                    codepoint: cell.c as u32,
                    fg: color_to_u32(cell.fg, colors),
                    bg: color_to_u32(cell.bg, colors),
                    attrs,
                    // shortcut: OSC 8 links need a per-snapshot uri table that the
                    // protocol does not carry yet, so M0 reports "no link".
                    link: 0,
                    width,
                }
            })
            .collect()
    }
}

impl Term for AlacrittyTerm {
    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let mut size = self.size.lock().map(|size| *size).unwrap_or(GridSize {
            cols,
            rows,
            history: 0,
        });
        size.cols = cols.max(2);
        size.rows = rows.max(1);
        if let Ok(mut slot) = self.size.lock() {
            *slot = size;
        }
        self.term.resize(size);
        self.term.reset_damage();
    }

    fn snapshot(&self) -> Snapshot {
        let size = self.size.lock().map(|size| *size).unwrap_or(GridSize {
            cols: 80,
            rows: 24,
            history: 0,
        });
        let colors = self.term.colors();
        let content = self.term.renderable_content();

        let screen = (0..size.rows)
            .map(|row| self.convert_row(Line(row as i32), colors))
            .collect();

        let history = if size.history == 0 {
            Vec::new()
        } else {
            let topmost = self.term.grid().topmost_line().0;
            // Newest-first in alacritty's storage; emit oldest-first.
            (topmost..0)
                .map(|line| self.convert_row(Line(line), colors))
                .collect()
        };

        Snapshot {
            cols: size.cols,
            rows: size.rows,
            alt: self.term.mode().contains(TermMode::ALT_SCREEN),
            cursor: cursor_of(content.cursor.shape, content.cursor.point),
            // alacritty keeps DECSTBM private; M0 reports the full screen and
            // documents the gap instead of guessing wrong values.
            scroll: (0, size.rows.saturating_sub(1)),
            screen,
            history,
        }
    }

    fn take_damage(&mut self) -> Vec<u16> {
        let rows: Vec<u16> = match self.term.damage() {
            TermDamage::Full => (0..self.size.lock().map(|size| size.rows).unwrap_or(24)).collect(),
            TermDamage::Partial(lines) => lines
                .map(|bounds| bounds.line.min(u16::MAX as usize) as u16)
                .collect(),
        };
        self.term.reset_damage();
        rows
    }

    fn set_palette(&mut self, palette: Option<Palette>) {
        if let Ok(mut slot) = self.palette.lock() {
            *slot = palette;
        }
    }

    fn title(&self) -> Option<String> {
        self.title.lock().ok().and_then(|slot| slot.clone())
    }

    fn take_replies(&mut self) -> Vec<u8> {
        self.replies
            .lock()
            .map(|mut out| std::mem::take(&mut *out))
            .unwrap_or_default()
    }
}

fn cursor_of(shape: CursorShape, point: Point) -> Cursor {
    let visible = shape != CursorShape::Hidden;
    Cursor {
        x: point.column.0.min(u16::MAX as usize) as u16,
        y: point.line.0.clamp(0, u16::MAX as i32) as u16,
        visible,
        shape: match shape {
            CursorShape::Underline => 1,
            CursorShape::Beam => 2,
            _ => 0,
        },
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
