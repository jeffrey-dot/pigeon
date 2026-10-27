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
pub trait Term {
    /// Parses bytes; replies (DA/DSR/…) are queued for [`Term::take_replies`].
    fn feed(&mut self, bytes: &[u8]);
    fn resize(&mut self, cols: u16, rows: u16);
    fn snapshot(&self) -> Snapshot;
    /// Row indices changed since the last call; empty when clean.
    fn take_damage(&mut self) -> Vec<u16>;
    /// Bytes the application expects on its PTY (query answers).
    fn take_replies(&mut self) -> Vec<u8>;
}

#[cfg(test)]
mod tests {
    use super::{alacritty::AlacrittyTerm, Term};

    fn term() -> AlacrittyTerm {
        AlacrittyTerm::new(20, 5, 100)
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
