//! `alacritty_terminal` behind the [`Term`](super::Term) trait.
//!
//! Nothing above this file knows about alacritty's types: snapshots are converted
//! to our own cells here, so swapping engines never reaches the ABI or protocol.

use std::sync::{Arc, Mutex};

use crate::term::Palette;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term as VtTerm, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor, Rgb};

use super::hyperlink::LinkExport;
use super::osc8_filter::Osc8Filter;
use super::sync_timeout::SyncTimeout;
use super::{Cell, Cursor, Snapshot, Term};

#[path = "alacritty_cell.rs"]
mod cell;
#[path = "alacritty_text.rs"]
mod text;

#[cfg(test)]
thread_local! {
    pub(crate) static CONVERTED_HISTORY_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
    /// The palette the host gave this pane (it sets one at birth: the client's theme or the
    /// default dark one). `None` means the caller explicitly cleared it — then `OSC 4/10/11`
    /// stay unanswered.
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
    parser: Processor<SyncTimeout>,
    links: Osc8Filter,
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
            // Programs push `CSI > flags u`; the client encodes keys by the active flags.
            kitty_keyboard: true,
            ..Config::default()
        };
        Self {
            term: VtTerm::new(config, &size, sink),
            parser: Processor::<SyncTimeout>::new(),
            links: Osc8Filter::default(),
            replies,
            size: size_cell,
            title,
            palette,
        }
    }

    /// `cols` comes from the caller, which reads the size once per picture rather than
    /// locking it for every row.
    fn convert_row(&self, line: Line, colors: &Colors, cols: u16, links: &mut LinkExport) -> Vec<Cell> {
        #[cfg(test)]
        if line.0 < 0 {
            CONVERTED_HISTORY_ROWS.set(CONVERTED_HISTORY_ROWS.get() + 1);
        }
        let grid = self.term.grid();
        (0..usize::from(cols))
            .map(|col| {
                let cell = &grid[Point::new(line, Column(col))];
                cell::convert(cell, colors, links)
            })
            .collect()
    }

    fn size(&self) -> GridSize {
        self.size.lock().map(|size| *size).unwrap_or(GridSize {
            cols: 80,
            rows: 24,
            history: 0,
        })
    }

    /// One picture of the pane with the newest `max_history` scrollback rows; 0 skips
    /// converting the scrollback.
    fn picture(&self, max_history: usize) -> Snapshot {
        let size = self.size();
        let colors = self.term.colors();
        let mut links = LinkExport::default();
        let screen = (0..size.rows)
            .map(|row| self.convert_row(Line(i32::from(row)), colors, size.cols, &mut links))
            .collect();
        let history = if size.history == 0 || max_history == 0 {
            Vec::new()
        } else {
            let topmost = self.term.grid().topmost_line().0;
            // History lines run from `topmost` (oldest) to -1 (newest); keep the newest.
            let available = usize::try_from(-topmost).unwrap_or(0);
            let skip = available.saturating_sub(max_history);
            let start = topmost.saturating_add(i32::try_from(skip).unwrap_or(i32::MAX));
            (start..0)
                .map(|line| self.convert_row(Line(line), colors, size.cols, &mut links))
                .collect()
        };
        let mut snapshot = self.frame(size);
        snapshot.screen = screen;
        snapshot.history = history;
        snapshot
    }

    /// Everything but the grid: size, cursor, alt screen, scroll region.
    fn frame(&self, size: GridSize) -> Snapshot {
        let content = self.term.renderable_content();
        Snapshot {
            cols: size.cols,
            rows: size.rows,
            alt: self.term.mode().contains(TermMode::ALT_SCREEN),
            cursor: cursor_of(content.cursor.shape, content.cursor.point),
            // alacritty keeps DECSTBM private; M0 reports the full screen and
            // documents the gap instead of guessing wrong values.
            scroll: (0, size.rows.saturating_sub(1)),
            screen: Vec::new(),
            history: Vec::new(),
        }
    }
}

impl Term for AlacrittyTerm {
    fn text_info(&self) -> crate::PgResult<super::text_query::TextInfo> { Ok(self.query_info()) }
    fn text_row(&self, row: i32) -> (u16, bool) { self.query_row(row) }
    fn text_cell(&self, row: i32, col: u16) -> (String, u8) { self.query_cell(row, col) }
    fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, &self.links.filter(bytes));
    }

    fn sync_deadline(&self) -> Option<std::time::Instant> {
        self.parser.sync_timeout().deadline()
    }

    fn flush_sync(&mut self) {
        self.parser.stop_sync(&mut self.term);
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
        self.picture(usize::MAX)
    }

    fn screen_snapshot(&self) -> Snapshot {
        self.picture(0)
    }

    fn snapshot_with_history_budget(&self, bytes: usize) -> Snapshot {
        let grid = self.term.grid();
        let cols = self.size().cols;
        let mut budget = super::history_budget::HistoryBudget::new(bytes);
        let keep = (grid.topmost_line().0..0).rev().take_while(|line| {
            budget.take_row(cols, (0..usize::from(cols)).flat_map(|col| {
                let cell = &grid[Point::new(Line(*line), Column(col))];
                [cell::ExportedText::new(cell).extra_len(), cell.hyperlink().map(|link| link.uri().len())]
                    .into_iter().flatten()
            }))
        }).count();
        self.picture(keep)
    }

    fn screen_rows(&self, rows: &[u16]) -> (Vec<(u16, Vec<Cell>)>, Snapshot) {
        let size = self.size();
        let colors = self.term.colors();
        let mut links = LinkExport::default();
        let picked = rows
            .iter()
            .filter(|y| **y < size.rows)
            .map(|y| (*y, self.convert_row(Line(i32::from(*y)), colors, size.cols, &mut links)))
            .collect();
        (picked, self.frame(size))
    }

    fn scrolled_snapshot(&self, offset: usize) -> (Snapshot, usize) {
        let mut view = self.picture(0);
        let total = (-self.term.grid().topmost_line().0).max(0) as usize;
        let offset = offset.min(total);
        if offset == 0 {
            return (view, total);
        }
        let colors = self.term.colors();
        let mut links = LinkExport::default();
        // Screen line 0 is `Line(0)`; history runs to `Line(-total)`. Shifting the window up
        // by `offset` reads exactly `rows` lines, so a step costs one screen, not 10k lines.
        view.screen = (0..i32::from(view.rows))
            .map(|row| self.convert_row(Line(row - offset as i32), colors, view.cols, &mut links))
            .collect();
        // The cursor belongs to the live screen; drawing it over history would lie.
        view.cursor.visible = false;
        (view, total)
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

    fn modes(&self) -> u32 {
        use super::{
            MODE_ALTERNATE_SCROLL, MODE_APP_CURSOR, MODE_APP_KEYPAD, MODE_BRACKETED_PASTE,
            MODE_CURSOR_BLINK, MODE_FOCUS_EVENTS, MODE_KITTY_ALL_KEYS, MODE_KITTY_ALTERNATE_KEYS,
            MODE_KITTY_ASSOCIATED_TEXT, MODE_KITTY_DISAMBIGUATE, MODE_KITTY_EVENT_TYPES,
            MODE_MOUSE_CLICK, MODE_MOUSE_DRAG, MODE_MOUSE_MOTION, MODE_MOUSE_SGR,
        };
        let mode = self.term.mode();
        [
            (TermMode::APP_CURSOR, MODE_APP_CURSOR),
            (TermMode::BRACKETED_PASTE, MODE_BRACKETED_PASTE),
            (TermMode::MOUSE_REPORT_CLICK, MODE_MOUSE_CLICK),
            (TermMode::MOUSE_DRAG, MODE_MOUSE_DRAG),
            (TermMode::MOUSE_MOTION, MODE_MOUSE_MOTION),
            (TermMode::SGR_MOUSE, MODE_MOUSE_SGR),
            (TermMode::FOCUS_IN_OUT, MODE_FOCUS_EVENTS),
            (TermMode::APP_KEYPAD, MODE_APP_KEYPAD),
            (TermMode::ALTERNATE_SCROLL, MODE_ALTERNATE_SCROLL),
            (TermMode::DISAMBIGUATE_ESC_CODES, MODE_KITTY_DISAMBIGUATE),
            (TermMode::REPORT_EVENT_TYPES, MODE_KITTY_EVENT_TYPES),
            (TermMode::REPORT_ALTERNATE_KEYS, MODE_KITTY_ALTERNATE_KEYS),
            (TermMode::REPORT_ALL_KEYS_AS_ESC, MODE_KITTY_ALL_KEYS),
            (TermMode::REPORT_ASSOCIATED_TEXT, MODE_KITTY_ASSOCIATED_TEXT),
        ]
        .iter()
        .filter(|(flag, _)| mode.contains(*flag))
        .fold(0, |bits, (_, bit)| bits | bit)
            | if self.term.cursor_style().blinking {
                MODE_CURSOR_BLINK
            } else {
                0
            }
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
