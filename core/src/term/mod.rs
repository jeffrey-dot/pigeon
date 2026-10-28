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
    /// harp's built-in dark theme (`dark_theme()` + `DARK_MODE_*` colours): what the host
    /// answers with before any client has told it otherwise. Once a client sends
    /// `appearance`, that palette is the only source — this is a pre-client fallback, not a
    /// second palette the panes switch between.
    pub fn default_dark() -> Self {
        Self::from_rgb(
            0x00ff_ffff,
            0x0005_0505,
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
    /// Row indices changed since the last call; empty when clean.
    fn take_damage(&mut self) -> Vec<u16>;
    /// Bytes the application expects on its PTY (query answers).
    fn take_replies(&mut self) -> Vec<u8>;

    /// Title the program announced (OSC 0/2), if any.
    fn title(&self) -> Option<String>;

    /// The palette used to answer colour queries; `None` keeps them unanswered.
    fn set_palette(&mut self, palette: Option<Palette>);
}

#[cfg(test)]
mod tests {
    use super::{alacritty::AlacrittyTerm, Palette, Term};

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
    fn the_default_palette_is_harps_dark_theme() {
        let mut term = term();
        term.set_palette(Some(Palette::default_dark()));
        term.feed(b"\x1b]11;?\x1b\\");
        term.feed(b"\x1b]10;?\x1b\\");
        term.feed(b"\x1b]4;1;?\x1b\\");
        let replies = String::from_utf8(term.take_replies()).expect("utf8");
        assert!(replies.contains("]11;rgb:0505/0505/0505"), "{replies:?}");
        assert!(replies.contains("]10;rgb:ffff/ffff/ffff"), "{replies:?}");
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
