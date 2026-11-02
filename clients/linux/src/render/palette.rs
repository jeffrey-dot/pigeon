//! Cell colours: the wire's `0x01000000 | idx` sentinel, xterm's 256-colour cube, and the
//! dark theme (protocol §3.3, `docs/renderer.md` §5).
//!
//! The values are the same ones the macOS client resolves from harp's theme files
//! (`TerminalPalette`), so a session looks the same on both platforms. Only the dark
//! appearance exists here for now; light mode needs a theme source the Linux side does not
//! have yet (GTK's `prefer-dark` tells us which one to ask for later).

use pigeon_core::api::{ATTR_REVERSE, ATTR_STRIKETHROUGH, ATTR_UNDERLINE};

/// `harp`'s `DARK_MODE_NORMAL_COLORS` + `DARK_MODE_BRIGHT_COLORS`.
const ANSI: [u32; 16] = [
    0x616161, 0xFF8272, 0xB4FA72, 0xFEFDC2, 0xA5D5FE, 0xFF8FFD, 0xD0D1FE, 0xF1F1F1, 0x8E8E8E,
    0xFFC4BD, 0xD6FCB9, 0xFEFDD5, 0xC1E3FE, 0xFFB1FE, 0xE5E6FE, 0xFEFFFF,
];

const FOREGROUND: u32 = 0xE6E6E6;
const BACKGROUND: u32 = 0x0E0F11;

/// One frame's colours as `0xRRGGBB`.
#[derive(Clone, Copy)]
pub struct Palette {
    pub foreground: u32,
    pub background: u32,
    pub ansi: [u32; 16],
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            foreground: FOREGROUND,
            background: BACKGROUND,
            ansi: ANSI,
        }
    }

    /// A cell's `(fg, bg)` with reverse video applied; `0` means "theme default".
    pub fn colors(&self, fg: u32, bg: u32, attrs: u32) -> (u32, u32) {
        let mut front = if fg == 0 {
            self.foreground
        } else {
            self.rgb(fg)
        };
        let mut back = if bg == 0 {
            self.background
        } else {
            self.rgb(bg)
        };
        if attrs & ATTR_REVERSE != 0 {
            std::mem::swap(&mut front, &mut back);
        }
        (front, back)
    }

    /// `0xRRGGBB` for a wire colour value; palette indices go through the ANSI table and
    /// xterm's 256-colour cube, exactly like `CellPalette.rgb` on macOS.
    pub fn rgb(&self, value: u32) -> u32 {
        if value & 0xff00_0000 != 0x0100_0000 {
            return value & 0x00ff_ffff;
        }
        let index = (value & 0xff) as usize;
        if index < 16 {
            return self.ansi[index];
        }
        if index < 232 {
            const STEPS: [u32; 6] = [0, 95, 135, 175, 215, 255];
            let cube = index - 16;
            return (STEPS[(cube / 36) % 6] << 16) | (STEPS[(cube / 6) % 6] << 8) | STEPS[cube % 6];
        }
        let level = 8 + (index as u32 - 232) * 10;
        (level << 16) | (level << 8) | level
    }

    /// Straight-alpha RGBA in 0…1 for the GPU.
    pub fn components(value: u32) -> [f32; 4] {
        [
            ((value >> 16) & 0xff) as f32 / 255.0,
            ((value >> 8) & 0xff) as f32 / 255.0,
            (value & 0xff) as f32 / 255.0,
            1.0,
        ]
    }
}

/// Attributes whose decoration is a rectangle under/through the text.
pub fn decoration(attrs: u32) -> Option<u32> {
    if attrs & ATTR_UNDERLINE != 0 {
        Some(ATTR_UNDERLINE)
    } else if attrs & ATTR_STRIKETHROUGH != 0 {
        Some(ATTR_STRIKETHROUGH)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_indexes_the_palette() {
        let palette = Palette::dark();
        assert_eq!(palette.rgb(0x0100_0000 | 2), palette.ansi[2]);
        assert_eq!(palette.rgb(0x0100_0000 | 9), palette.ansi[9]);
    }

    #[test]
    fn plain_rgb_passes_through() {
        let palette = Palette::dark();
        assert_eq!(palette.rgb(0x12_34_56), 0x12_34_56);
    }

    #[test]
    fn cube_and_greyscale_follow_xterm() {
        let palette = Palette::dark();
        assert_eq!(palette.rgb(0x0100_0010), 0x000000);
        assert_eq!(palette.rgb(0x0100_00e7), 0xffffff);
        assert_eq!(palette.rgb(0x0100_00ff), 0xeeeeee);
    }

    #[test]
    fn reverse_swaps_the_pair() {
        let palette = Palette::dark();
        let (fg, bg) = palette.colors(0, 0, ATTR_REVERSE);
        assert_eq!((fg, bg), (palette.background, palette.foreground));
    }
}
