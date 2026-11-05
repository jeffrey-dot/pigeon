//! Harp-aligned colour tokens and fonts, read from the Harp source at the pinned revision
//! `54c5286078484ab38304bf8be1052f9c60f26a90` (`app/src/workspace/view/vertical_tabs.rs` colour
//! tokens and sizing, `app/src/themes/default_themes.rs` terminal fg/bg). Only the token values
//! and sizes are borrowed; the drawing is ours (no Harp implementation code is copied).
//!
//! GPUI draws every surface itself, so unlike the GTK client there is no system CSS: these
//! constants are the single source of the look.

use gpui::{Rgba, WindowAppearance};

/// Whether to use the dark palette. `PIGEON_APPEARANCE=light|dark` forces it (used by the
/// capture harness for deterministic output); otherwise it follows the window's appearance.
pub fn dark_for(appearance: WindowAppearance) -> bool {
    match std::env::var("PIGEON_APPEARANCE").as_deref() {
        Ok("light") => false,
        Ok("dark") => true,
        // Follow the system; default to light (the reference screenshot) when unknown.
        _ => matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    }
}

/// Fonts, pinned so the look does not drift across platforms. The sidebar and card header use the
/// platform UI font (Harp loads the macOS system font for its sidebar); the terminal grid uses a
/// named monospace family that is known to exist on each platform. `SF Mono` is not resolvable by
/// name on macOS (CoreText falls back to Helvetica), so macOS uses Menlo.
pub mod fonts {
    use gpui::{FontFeatures, SharedString};
    use std::sync::Arc;

    /// Sidebar / card header UI text.
    pub fn ui() -> SharedString {
        SharedString::new_static(".SystemUIFont")
    }

    /// The terminal grid.
    pub fn mono() -> SharedString {
        if cfg!(target_os = "macos") {
            SharedString::new_static("Menlo")
        } else {
            SharedString::new_static("DejaVu Sans Mono")
        }
    }

    /// Tabular (fixed-width) digits for the relative-time column, so `1m` and `12m` line up.
    pub fn tabular() -> FontFeatures {
        FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
    }

    pub const TERMINAL_SIZE: f32 = 13.0;
    pub const TERMINAL_LINE_HEIGHT: f32 = 17.0;
}

/// One resolved palette. Fields map to the Harp `colors` token table (sidebar) and
/// `light_theme`/`dark_theme` (terminal).
#[derive(Clone, Copy)]
pub struct Palette {
    // Text.
    pub ink: Rgba,
    /// Harp `SUB`: row time, idle row titles.
    pub sub: Rgba,
    pub gray: Rgba,
    pub section_name: Rgba,
    pub section_count: Rgba,
    pub needs_text: Rgba,
    // Status-icon inks.
    pub icon_needs_ring: Rgba,
    pub icon_needs_dot: Rgba,
    pub icon_strong: Rgba,
    pub icon_on_fill: Rgba,
    pub ring: Rgba,
    pub track: Rgba,
    pub moon: Rgba,
    // Surfaces.
    pub selected: Rgba,
    pub selected_edge: Rgba,
    pub selected_shadow: Rgba,
    pub hover: Rgba,
    /// Harp's `GLASS_TINT` composited over its opaque `SIDEBAR`: what the sidebar shows without
    /// a live window blur (kept opaque so captures are deterministic).
    pub sidebar_surface: Rgba,
    pub divider: Rgba,
    pub card_edge: Rgba,
    pub card_shadow: Rgba,
    pub terminal_bg: Rgba,
}

impl Palette {
    pub fn resolve(dark: bool) -> Self {
        if dark {
            Self {
                ink: rgb(0xf5f5f7),
                sub: rgb(0x98989f),
                gray: rgb(0x8e8e93),
                section_name: rgb(0x8e8e93),
                section_count: rgb(0x636366),
                needs_text: rgb(0xf0b35c),
                icon_needs_ring: rgb(0xe8a33d),
                icon_needs_dot: rgb(0xf0b35c),
                icon_strong: rgb(0xe5e5ea),
                icon_on_fill: rgb(0x1c1c1e),
                ring: rgb(0x636366),
                track: rgb(0x3a3a3e),
                moon: rgb(0x8e8e93),
                selected: rgb(0x3a3a3d),
                selected_edge: rgba(0xffffff, 26),
                selected_shadow: rgba(0x000000, 64),
                hover: rgba(0xffffff, 15),
                sidebar_surface: over(rgba(0x1e1e20, 140), rgb(0x232326)),
                divider: rgb(0x3a3a3c),
                card_edge: rgba(0xffffff, 26),
                card_shadow: rgba(0x000000, 64),
                terminal_bg: rgb(0x050505),
            }
        } else {
            Self {
                ink: rgb(0x1d1d1f),
                sub: rgb(0x626269),
                gray: rgb(0x6e6e73),
                section_name: rgb(0x7c7c82),
                section_count: rgb(0xa1a1a6),
                needs_text: rgb(0x9a5700),
                icon_needs_ring: rgb(0xc98314),
                icon_needs_dot: rgb(0xe8a33d),
                icon_strong: rgb(0x3a3a3c),
                icon_on_fill: rgb(0xffffff),
                ring: rgb(0xa1a1a6),
                track: rgb(0xe0e0e4),
                moon: rgb(0x8e8e93),
                selected: rgb(0xffffff),
                selected_edge: rgba(0x000000, 20),
                selected_shadow: rgba(0x000000, 15),
                hover: rgba(0x000000, 13),
                sidebar_surface: over(rgba(0xfafafc, 140), rgb(0xe8e8eb)),
                divider: rgb(0xececef),
                card_edge: rgba(0x000000, 26),
                card_shadow: rgba(0x000000, 13),
                terminal_bg: rgb(0xffffff),
            }
        }
    }
}

/// Layout constants, from the Harp sidebar `const` module and `docs/ui.md` §2–§3, in logical
/// pixels.
pub mod metrics {
    pub const SIDEBAR_WIDTH: f32 = 272.0;
    pub const HEADER_HEIGHT: f32 = 56.0;
    pub const LIST_SIDE_PADDING: f32 = 10.0;
    pub const LIST_TOP_PADDING: f32 = 4.0;
    pub const SECTION_GAP: f32 = 14.0;
    pub const SECTION_HEADER_HEIGHT: f32 = 28.0;
    pub const SECTION_TITLE_SIZE: f32 = 11.5;
    pub const ROW_HEIGHT: f32 = 30.0;
    pub const ROW_GAP: f32 = 1.0;
    pub const ROW_TITLE_SIZE: f32 = 13.0;
    pub const ROW_RADIUS: f32 = 8.0;
    pub const ROW_PAD_LEFT: f32 = 12.0;
    pub const ROW_PAD_RIGHT: f32 = 8.0;
    /// Section header right padding (Harp `section_header_content`).
    pub const SECTION_PAD_RIGHT: f32 = 9.0;
    pub const SECTION_SPACING: f32 = 6.0;
    pub const NEEDS_SIZE: f32 = 12.0;
    pub const ROW_GAP_INNER: f32 = 8.0;
    /// Harp `row_time_text`; the reference image measures this size, not ui.md's former 10.
    pub const TIME_SIZE: f32 = 11.5;
    pub const STATUS_SLOT: f32 = 16.0;
    pub const STATUS_GLYPH: f32 = 15.0;
    // Card.
    pub const CARD_INSET: f32 = 8.0;
    pub const CARD_RADIUS: f32 = 10.0;
    pub const CARD_STROKE: f32 = 0.5;
    pub const CARD_HEADER_HEIGHT: f32 = 40.0;
    pub const CARD_HEADER_PAD_OPEN: f32 = 32.0;
    pub const HEADER_PAD_RIGHT: f32 = 20.0;
    pub const CARD_STATUS_SIZE: f32 = 12.0;
    pub const CARD_TITLE_SIZE: f32 = 12.5;
    pub const CARD_TITLE_SPACING: f32 = 10.0;
    pub const CARD_STATUS_MARGIN: f32 = 12.0;
    pub const TERMINAL_PAD_LEFT: f32 = 32.0;
    // Top-bar buttons.
    pub const TOP_BUTTON_SIZE: f32 = 28.0;
    pub const TOP_BUTTON_RADIUS: f32 = 7.0;
    pub const TOP_BUTTON_GLYPH: f32 = 17.0;
    pub const TOP_BUTTON_GAP: f32 = 2.0;
    pub const TOP_BAR_PAD_RIGHT: f32 = 14.0;
    /// Traffic-light origin, so their centre sits on the 56 pt top bar's centre line.
    pub const TRAFFIC_LIGHT_X: f32 = 18.0;
    pub const TRAFFIC_LIGHT_Y: f32 = 21.0;
    pub const MIN_CONTENT_WIDTH: f32 = 640.0;
    pub const MIN_CONTENT_HEIGHT: f32 = 400.0;
}

/// An opaque sRGB colour.
const fn rgb(hex: u32) -> Rgba {
    rgba(hex, 255)
}

/// An sRGB colour with an 0–255 alpha, matching how the Harp tokens write overlays.
const fn rgba(hex: u32, alpha: u8) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: alpha as f32 / 255.0,
    }
}

/// Source-over compositing of `top` on an opaque `bottom`.
const fn over(top: Rgba, bottom: Rgba) -> Rgba {
    let a = top.a;
    Rgba {
        r: top.r * a + bottom.r * (1.0 - a),
        g: top.g * a + bottom.g * (1.0 - a),
        b: top.b * a + bottom.b * (1.0 - a),
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(c: Rgba) -> u32 {
        let byte = |v: f32| (v * 255.0).round() as u32;
        (byte(c.r) << 16) | (byte(c.g) << 8) | byte(c.b)
    }

    #[test]
    fn light_sidebar_surface_matches_the_reference_screenshot() {
        // docs/images/reference-harp-layout.png samples #f2f2f3 in the empty sidebar.
        assert_eq!(hex(Palette::resolve(false).sidebar_surface), 0xf2f2f4);
    }

    #[test]
    fn explicit_appearance_overrides_the_system() {
        std::env::set_var("PIGEON_APPEARANCE", "light");
        assert!(!dark_for(WindowAppearance::Dark));
        std::env::set_var("PIGEON_APPEARANCE", "dark");
        assert!(dark_for(WindowAppearance::Light));
        std::env::remove_var("PIGEON_APPEARANCE");
        assert!(!dark_for(WindowAppearance::Light));
        assert!(dark_for(WindowAppearance::VibrantDark));
    }
}
