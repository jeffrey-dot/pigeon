//! Cell colours: the wire's `0x01000000 | idx` sentinel, xterm's 256-colour cube, and the
//! light/dark themes (protocol §3.3, `docs/renderer.md` §5).
//!
//! The values are the same ones the macOS client resolves from harp's theme files
//! (`TerminalPalette`); light and dark appearances share those tokens.

use pigeon_core::api::{ATTR_REVERSE, ATTR_STRIKETHROUGH, ATTR_UNDERLINE};

/// `harp`'s `DARK_MODE_NORMAL_COLORS` + `DARK_MODE_BRIGHT_COLORS`.
const ANSI: [u32; 16] = [
    0x616161, 0xFF8272, 0xB4FA72, 0xFEFDC2, 0xA5D5FE, 0xFF8FFD, 0xD0D1FE, 0xF1F1F1, 0x8E8E8E,
    0xFFC4BD, 0xD6FCB9, 0xFEFDD5, 0xC1E3FE, 0xFFB1FE, 0xE5E6FE, 0xFEFFFF,
];

const FOREGROUND: u32 = 0xFFFFFF;
const BACKGROUND: u32 = 0x050505;

/// One frame's colours as `0xRRGGBB`.
#[derive(Clone, Copy)]
pub struct Palette {
    pub foreground: u32,
    pub background: u32,
    pub ansi: [u32; 16],
}

impl Palette {
    pub fn light() -> Self {
        Self {
            foreground: 0x111111,
            background: 0xffffff,
            ansi: [
                0x212121, 0xc30771, 0x10a778, 0x8a7f00, 0x008ec4, 0x523c79, 0x15828f, 0xe0e0e0,
                0x212121, 0xfb007a, 0x1e9e77, 0x9a8c00, 0x1a8fc4, 0x6855de, 0x1f8a9a, 0xf1f1f1,
            ],
        }
    }

    pub fn dark() -> Self {
        Self {
            foreground: FOREGROUND,
            background: BACKGROUND,
            ansi: ANSI,
        }
    }

    /// A cell's `(fg, bg)` with reverse video applied; `0` means "theme default".
    pub fn colors(&self, fg: u32, bg: u32, attrs: u32) -> (u32, u32) {
        let mut front = if fg == 0 || (fg == 0xe5e5e5 && self.background == 0xffffff) {
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
        if self.background == 0xffffff && back == self.background {
            // Named default foreground is exported as grey RGB by the core. Keep it and
            // light neutral ANSI text readable on a white surface, as on macOS.
            let r = (front >> 16) & 255;
            let g = (front >> 8) & 255;
            let b = front & 255;
            if r.max(g).max(b) - r.min(g).min(b) < 31 && r + g + b > 420 {
                front = 0x66666b;
            }
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

    #[test]
    fn light_defaults_and_neutral_ansi_stay_readable_on_white() {
        let palette = Palette::light();
        assert_eq!(palette.colors(0, 0, 0), (0x111111, 0xffffff));
        assert_eq!(palette.colors(0xe5e5e5, 0, 0), (0x111111, 0xffffff));
        assert_eq!(palette.colors(0x01000007, 0, 0).0, 0x66666b);
        assert_eq!(palette.colors(0x123456, 0x987654, 0), (0x123456, 0x987654));
    }
}
