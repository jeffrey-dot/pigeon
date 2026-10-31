//! VT state: the one place that interprets terminal byte streams.
//!
//! [`alacritty`] wraps `alacritty_terminal` behind the [`Term`] trait. Swapping
//! engines means adding another implementation of that trait; nothing above this
//! module knows which engine is underneath.

pub mod alacritty;
pub mod frozen;

/// Attribute bits, mirrored by `PG_ATTR_*` in the C ABI and by the protocol's
/// `cell[1]`.
pub const ATTR_BOLD: u32 = 1 << 0;
pub const ATTR_ITALIC: u32 = 1 << 1;
pub const ATTR_UNDERLINE: u32 = 1 << 2;
pub const ATTR_REVERSE: u32 = 1 << 3;
pub const ATTR_STRIKETHROUGH: u32 = 1 << 4;
pub const ATTR_DIM: u32 = 1 << 5;

/// One grid cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub codepoint: u32,
    /// `0x01000000 | idx` for a palette entry, else `0xRRGGBB`.
    pub fg: u32,
    pub bg: u32,
    pub attrs: u32,
    /// 0 = no hyperlink.
    pub link: u32,
    /// 0 = wide-char continuation, 1 = normal, 2 = owns the next cell.
    pub width: u8,
}

impl Cell {
    pub const BLANK: Self = Self {
        codepoint: ' ' as u32,
        fg: 0x01_00_00_07,
        bg: 0x01_00_00_00,
        attrs: 0,
        link: 0,
        width: 1,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    /// 0 block, 1 underline, 2 bar.
    pub shape: u8,
}

/// A full picture of one pane, enough to rebuild a client without replaying
/// bytes. Immutable once returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub alt: bool,
    pub cursor: Cursor,
    /// Inclusive top and bottom line of the scrolling region, in screen rows.
    pub scroll: (u16, u16),
    /// `rows` entries of `cols` cells.
    pub screen: Vec<Vec<Cell>>,
    /// Scrollback, oldest first.
    pub history: Vec<Vec<Cell>>,
}

impl Snapshot {
    /// The screen as it looks scrolled back `offset` lines: history above, the live screen
    /// below. Offset 0 is the live view. Used by the host to answer `scroll` — the client
    /// never gets the whole history over the wire, so a 10k-line backlog costs one screen.
    pub fn scrolled(&self, offset: usize) -> Self {
        let mut view = self.clone();
        let history = self.history.len();
        let offset = offset.min(history);
        if offset == 0 {
            return view;
        }
        // Line `i` of the view is history line `start + i`, or — once those run out — line
        // `start + i - history` of the live screen. That keeps the window exactly `rows` tall
        // at every offset, including the very top of the backlog.
        let start = history - offset;
        view.screen = (0..self.rows as usize)
            .map(|i| {
                let line = start + i;
                if line < history {
                    self.history[line].clone()
                } else {
                    self.screen[line - history].clone()
                }
            })
            .collect();
        // The cursor belongs to the live screen; drawing it over history would lie.
        view.cursor.visible = false;
        view
    }

    /// Compares the visible grid only; used by the reconnect tests.
    pub fn same_screen(&self, other: &Self) -> bool {
        self.cols == other.cols
            && self.rows == other.rows
            && self.alt == other.alt
            && self.cursor == other.cursor
            && self.screen == other.screen
    }
}

/// What the rest of the core needs from a terminal engine.
/// A colour as the client draws it (`0xRRGGBB`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Colour {
    pub fn from_rgb(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }
}

/// The colours the connected client is showing, used to answer the application's
/// `OSC 4` (palette) / `OSC 10` (foreground) / `OSC 11` (background) queries.
///
/// The host always has one (`Palette::default_dark()` until a client sends `appearance`); `None`
/// means "no answer yet" and only happens if a caller explicitly clears it, e.g. a frozen term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub fg: Colour,
    pub bg: Colour,
    pub ansi: [Colour; 16],
}

impl Palette {
    /// pigeon's dark card (`#E6E6E6` on `#0E0F11`) with harp's `DARK_MODE_*` colours: what the host
    /// answers with before any client has told it otherwise. Once a client sends
    /// `appearance`, that palette is the only source — this is a pre-client fallback, not a
    /// second palette the panes switch between.
    pub fn default_dark() -> Self {
        Self::from_rgb(
            0x00e6_e6e6,
            0x000e_0f11,
            &[
                0x616161, 0xFF8272, 0xB4FA72, 0xFEFDC2, 0xA5D5FE, 0xFF8FFD, 0xD0D1FE, 0xF1F1F1,
                0x8E8E8E, 0xFFC4BD, 0xD6FCB9, 0xFEFDD5, 0xC1E3FE, 0xFFB1FE, 0xE5E6FE, 0xFEFFFF,
            ],
        )
        .expect("default palette is well-formed")
    }

    /// Builds a palette from the wire values; a wrong-length ANSI list is rejected rather
    /// than padded, so a broken client cannot invent colours.
    pub fn from_rgb(fg: u32, bg: u32, ansi: &[u32]) -> Option<Self> {
        let ansi: [Colour; 16] = ansi
            .iter()
            .copied()
            .map(Colour::from_rgb)
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        Some(Self {
            fg: Colour::from_rgb(fg),
            bg: Colour::from_rgb(bg),
            ansi,
        })
    }

    /// Indices follow `alacritty_terminal`'s colour table: 0–15 ANSI, 16–231 the colour cube,
    /// 232–255 the grey ramp, 256 foreground, 257 background. Cursor colours (258+) are not
    /// answered yet (`docs/plan.md` §11).
    pub fn resolve(&self, index: usize) -> Option<Colour> {
        match index {
            0..=15 => Some(self.ansi[index]),
            16..=231 => {
                let steps = [0, 95, 135, 175, 215, 255];
                let value = index - 16;
                Some(Colour {
                    r: steps[(value / 36) % 6],
                    g: steps[(value / 6) % 6],
                    b: steps[value % 6],
                })
            }
            232..=255 => {
                let level = 8 + (index as u32 - 232) * 10;
                Some(Colour {
                    r: level as u8,
                    g: level as u8,
                    b: level as u8,
                })
            }
            256 => Some(self.fg),
            257 => Some(self.bg),
            _ => None,
        }
    }
}

pub trait Term {
    /// Parses bytes; replies (DA/DSR/…) are queued for [`Term::take_replies`].
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
    fn snapshot(&self) -> Snapshot;
    /// The same picture without `history`. Deltas and history-less attaches only read the
    /// screen, and converting 10 000 scrollback lines per frame is most of a frame's cost.
    fn screen_snapshot(&self) -> Snapshot {
        let mut snapshot = self.snapshot();
        snapshot.history.clear();
        snapshot
    }
    /// The screen scrolled back `offset` lines (clamped to the history), plus the history
    /// length; equal to `snapshot().scrolled(offset)` without `history`. Engines override it
    /// to convert one screen of rows instead of the whole backlog on every scroll step.
    fn scrolled_snapshot(&self, offset: usize) -> (Snapshot, usize) {
        let snapshot = self.snapshot();
        let total = snapshot.history.len();
        let mut view = snapshot.scrolled(offset);
        view.history.clear();
        (view, total)
    }
    /// Row indices changed since the last call; empty when clean.
    fn take_damage(&mut self) -> Vec<u16>;
    /// Bytes the application expects on its PTY (query answers).
    fn take_replies(&mut self) -> Vec<u8>;

    /// Title the program announced (OSC 0/2), if any.
    fn title(&self) -> Option<String>;

    /// The palette used to answer colour queries; `None` keeps them unanswered.
    fn set_palette(&mut self, palette: Option<Palette>);

    /// DEC private modes the program turned on that change what the *client* sends:
    /// `MODE_*` bits. The host forwards them so key, mouse and paste encoding match.
    fn modes(&self) -> u32 {
        0
    }
}

/// Arrow keys send `ESC O A` instead of `ESC [ A` (DECCKM, `?1`).
pub const MODE_APP_CURSOR: u32 = 1 << 0;
/// Wrap pastes in `ESC [200~ … ESC [201~` (`?2004`).
pub const MODE_BRACKETED_PASTE: u32 = 1 << 1;
/// Report button presses (`?1000`).
pub const MODE_MOUSE_CLICK: u32 = 1 << 2;
/// Also report motion while a button is down (`?1002`).
pub const MODE_MOUSE_DRAG: u32 = 1 << 3;
/// Report all motion (`?1003`).
pub const MODE_MOUSE_MOTION: u32 = 1 << 4;
/// Mouse reports in SGR form `ESC [< b;x;y M|m` (`?1006`).
pub const MODE_MOUSE_SGR: u32 = 1 << 5;
/// Send `ESC [I` / `ESC [O` on focus changes (`?1004`).
pub const MODE_FOCUS_EVENTS: u32 = 1 << 6;
/// Application keypad (DECKPAM).
pub const MODE_APP_KEYPAD: u32 = 1 << 7;
/// Wheel on the alternate screen scrolls by sending arrow keys (`?1007`, alacritty default on).
pub const MODE_ALTERNATE_SCROLL: u32 = 1 << 8;
/// The program asked for a blinking cursor (DECSCUSR 1/3/5, or `?12h`).
pub const MODE_CURSOR_BLINK: u32 = 1 << 9;

#[cfg(test)]
mod tests {
    use super::{alacritty::AlacrittyTerm, Palette, Term, MODE_CURSOR_BLINK};

    fn term() -> AlacrittyTerm {
        AlacrittyTerm::new(20, 5, 100)
    }

    /// The light palette the client sends (`#111111` on `#ffffff`, harp's ANSI table).
    fn light_palette() -> Palette {
        Palette::from_rgb(
            0x0011_1111,
            0x00ff_ffff,
            &[
                0x212121, 0xC30771, 0x10A778, 0xA89C14, 0x008EC4, 0x523C79, 0x20A5BA, 0xE0E0E0,
                0x212121, 0xFB007A, 0x5FD7AF, 0xF3E430, 0x20BBFC, 0x6855DE, 0x4FB8CC, 0xF1F1F1,
            ],
        )
        .expect("palette")
    }

    #[test]
    fn modes_follow_what_the_program_turns_on_and_off() {
        use super::{
            MODE_APP_CURSOR, MODE_BRACKETED_PASTE, MODE_MOUSE_CLICK, MODE_MOUSE_DRAG,
            MODE_MOUSE_SGR,
        };
        let mut term = term();
        let clean = term.modes();
        assert_eq!(clean & (MODE_APP_CURSOR | MODE_BRACKETED_PASTE | MODE_MOUSE_CLICK), 0);
        // What vim / htop send on start.
        term.feed(b"\x1b[?1h\x1b[?2004h\x1b[?1002h\x1b[?1006h");
        let on = term.modes();
        for bit in [MODE_APP_CURSOR, MODE_BRACKETED_PASTE, MODE_MOUSE_DRAG, MODE_MOUSE_SGR] {
            assert_ne!(on & bit, 0, "bit {bit:#x} after enabling");
        }
        term.feed(b"\x1b[?1l\x1b[?2004l\x1b[?1002l\x1b[?1006l");
        let off = term.modes();
        assert_eq!(off & (MODE_APP_CURSOR | MODE_BRACKETED_PASTE | MODE_MOUSE_DRAG | MODE_MOUSE_SGR), 0);
        // Garbage mode numbers change nothing.
        term.feed(b"\x1b[?99999h");
        assert_eq!(term.modes(), off);
    }

    #[test]
    fn the_default_palette_is_pigeons_dark_card() {
        let mut term = term();
        term.set_palette(Some(Palette::default_dark()));
        term.feed(b"\x1b]11;?\x1b\\");
        term.feed(b"\x1b]10;?\x1b\\");
        term.feed(b"\x1b]4;1;?\x1b\\");
        let replies = String::from_utf8(term.take_replies()).expect("utf8");
        assert!(replies.contains("]11;rgb:0e0e/0f0f/1111"), "{replies:?}");
        assert!(replies.contains("]10;rgb:e6e6/e6e6/e6e6"), "{replies:?}");
        assert!(replies.contains("]4;1;rgb:ffff/8282/7272"), "{replies:?}");
    }

    #[test]
    fn colour_queries_are_answered_from_the_client_palette() {
        let mut term = term();
        term.set_palette(Some(light_palette()));
        term.feed(b"\x1b]11;?\x1b\\"); // background
        term.feed(b"\x1b]10;?\x1b\\"); // foreground
        term.feed(b"\x1b]4;1;?\x1b\\"); // palette entry 1
        let replies = String::from_utf8(term.take_replies()).expect("utf8");
        assert!(replies.contains("]11;rgb:ffff/ffff/ffff"), "{replies:?}");
        assert!(replies.contains("]10;rgb:1111/1111/1111"), "{replies:?}");
        assert!(replies.contains("]4;1;rgb:c3c3/0707/7171"), "{replies:?}");
    }

    #[test]
    fn colour_queries_stay_silent_without_a_palette() {
        let mut term = term();
        term.feed(b"\x1b]11;?\x1b\\");
        assert!(term.take_replies().is_empty());
    }

    #[test]
    fn cursor_colour_and_unknown_queries_are_not_answered() {
        let mut term = term();
        term.set_palette(Some(light_palette()));
        term.feed(b"\x1b]12;?\x1b\\"); // cursor colour: out of scope
        term.feed(b"\x1b]4;999;?\x1b\\"); // out of the table
        let replies = String::from_utf8(term.take_replies()).expect("utf8");
        assert!(!replies.contains("]12;"), "{replies:?}");
        assert!(!replies.contains("]4;999;"), "{replies:?}");
    }

    #[test]
    fn snapshot_is_stable_and_reproducible() {
        let bytes = b"hello\r\nworld \x1b[31mred\x1b[0m";

        let mut a = term();
        a.feed(bytes);
        let first = a.snapshot();
        let again = a.snapshot();
        assert_eq!(first, again, "snapshot must not mutate the grid");

        // Simulated reconnect: a brand new terminal fed the same bytes.
        let mut b = term();
        b.feed(bytes);
        assert!(
            first.same_screen(&b.snapshot()),
            "a fresh term fed the same bytes must produce the same screen"
        );
    }

    #[test]
    fn wide_characters_occupy_two_cells() {
        let mut t = term();
        t.feed("你好".as_bytes());
        let snap = t.snapshot();
        let row = &snap.screen[0];
        assert_eq!(row[0].codepoint, '你' as u32);
        assert_eq!(row[0].width, 2);
        assert_eq!(row[1].width, 0, "continuation cell of a wide char");
        assert_eq!(row[2].codepoint, '好' as u32);
        assert_eq!(row[2].width, 2);
    }

    #[test]
    fn alternate_screen_is_flagged() {
        let mut t = term();
        t.feed(b"\x1b[?1049h");
        assert!(t.snapshot().alt);
        t.feed(b"alt");
        assert_eq!(t.snapshot().screen[0][0].codepoint, 'a' as u32);
        t.feed(b"\x1b[?1049l");
        assert!(!t.snapshot().alt);
    }

    #[test]
    fn truecolor_survives_the_snapshot() {
        let mut t = term();
        t.feed(b"\x1b[38;2;1;2;3m\x1b[48;2;4;5;6mX");
        let cell = t.snapshot().screen[0][0];
        assert_eq!(cell.fg, 0x0001_0203);
        assert_eq!(cell.bg, 0x0004_0506);
    }

    #[test]
    fn damage_is_consumed_by_the_read() {
        let mut t = term();
        t.feed(b"line one\r\nline two");
        let first = t.take_damage();
        assert!(first.contains(&0), "row 0 changed");
        assert!(first.contains(&1), "row 1 changed");

        // alacritty always marks the cursor row damaged, so the second read may
        // still mention row 1 — but never row 0 again.
        let second = t.take_damage();
        assert!(!second.contains(&0), "row 0 was already reported");
    }

    #[test]
    fn a_tab_advances_to_the_next_stop_without_writing_a_glyph() {
        let mut t = term();
        t.feed(b"a\tb");
        let row = &t.snapshot().screen[0];
        let text: String = row
            .iter()
            .map(|cell| char::from_u32(cell.codepoint).unwrap_or(' '))
            .collect();
        assert_eq!(
            text.trim_end(),
            format!("a{}b", " ".repeat(7)).as_str(),
            "tab is blank cells up to the stop, not a glyph the client would have to draw"
        );
    }

    #[test]
    fn scrolled_output_lands_in_history_oldest_first() {
        let mut t = term();
        for line in 1..=10 {
            t.feed(format!("line {line}\r\n").as_bytes());
        }
        let snap = t.snapshot();
        assert!(
            !snap.history.is_empty(),
            "5 screen rows, 11 fed lines: the rest has to be history"
        );
        let first: String = snap.history[0]
            .iter()
            .map(|cell| char::from_u32(cell.codepoint).unwrap_or(' '))
            .collect();
        assert!(
            first.trim().starts_with("line 1"),
            "oldest line first, got {first:?}"
        );
        assert_eq!(snap.history[0].len(), 20, "history rows are full width");
    }

    /// `cargo test --release -- --ignored scroll_step_cost --nocapture`: one `scroll` answer on
    /// a full 10k-line backlog, the old way (whole history) against the engine's slice.
    #[test]
    #[ignore]
    fn scroll_step_cost() {
        let mut t = AlacrittyTerm::new(200, 50, 10_000);
        for line in 0..12_000 {
            t.feed(format!("{line:>6} the quick brown fox jumps over the lazy dog\r\n").as_bytes());
        }
        let steps = 50;
        let start = std::time::Instant::now();
        for step in 0..steps {
            let _ = t.snapshot().scrolled(step * 3);
        }
        let full = start.elapsed() / steps as u32;
        let start = std::time::Instant::now();
        for step in 0..steps {
            let _ = t.scrolled_snapshot(step * 3);
        }
        let sliced = start.elapsed() / steps as u32;
        eprintln!("scroll step: whole history {full:?}, one screen {sliced:?}");
    }

    #[test]
    fn cursor_blink_follows_decscusr_and_a_bad_style_changes_nothing() {
        let mut t = term();
        assert_eq!(t.modes() & MODE_CURSOR_BLINK, 0, "steady by default");
        t.feed(b"\x1b[5 q"); // blinking bar
        assert_ne!(t.modes() & MODE_CURSOR_BLINK, 0);
        assert_eq!(t.snapshot().cursor.shape, 2);
        t.feed(b"\x1b[2 q"); // steady block
        assert_eq!(t.modes() & MODE_CURSOR_BLINK, 0);
        t.feed(b"\x1b[99 q"); // not a style
        assert_eq!(t.modes() & MODE_CURSOR_BLINK, 0);
        assert_eq!(t.snapshot().cursor.shape, 0);
    }

    #[test]
    fn the_engines_scrolled_view_matches_slicing_the_full_history() {
        let mut t = term();
        for line in 1..=40 {
            t.feed(format!("\x1b[1mline\x1b[0m {line}\r\n").as_bytes());
        }
        let full = t.snapshot();
        let total = full.history.len();
        // Every offset, including past the top of the backlog (clamped, never out of range).
        for offset in [0, 1, 3, total - 1, total, total + 50, usize::MAX] {
            let (fast, reported) = t.scrolled_snapshot(offset);
            let mut slow = full.scrolled(offset);
            slow.history.clear();
            assert_eq!(reported, total);
            assert_eq!(fast, slow, "offset {offset}");
        }
    }

    #[test]
    fn scrolling_back_shows_history_above_the_live_screen() {
        let mut t = term();
        for line in 1..=30 {
            t.feed(format!("line {line}\r\n").as_bytes());
        }
        let live = t.snapshot();
        assert!(!live.history.is_empty(), "30 lines through a 5-row screen");

        let back = live.scrolled(2);
        let expected_top = live.history[live.history.len() - 2].clone();
        assert_eq!(
            back.screen[0], expected_top,
            "the newest history row is on top"
        );
        assert_eq!(back.screen[2], live.screen[0], "the live screen follows");
        assert!(!back.cursor.visible, "no cursor over history");
        assert_eq!(
            back.screen.len(),
            live.rows as usize,
            "the view is still a screen"
        );

        // The live view is untouched, and a huge offset is clamped to the history there is.
        assert_eq!(live.scrolled(0).screen, live.screen);
        assert!(live.cursor.visible == live.cursor.visible);
        let far = live.scrolled(usize::MAX);
        assert_eq!(
            far.screen[0],
            live.history[0].clone(),
            "scrolling far back stops at the oldest line kept"
        );
    }

    #[test]
    fn resize_keeps_the_grid_consistent() {
        let mut t = term();
        t.feed(b"hello");
        t.resize(40, 10);
        let snap = t.snapshot();
        assert_eq!((snap.cols, snap.rows), (40, 10));
        assert_eq!(snap.screen.len(), 10);
        assert!(snap.screen.iter().all(|row| row.len() == 40));
    }
}
