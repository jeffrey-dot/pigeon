import AppKit

/// The terminal's ANSI palette, copied from harp's theme files
/// (`app/src/themes/default_themes.rs:11-51`): 8 normal + 8 bright colours per appearance.
///
/// harp pairs each appearance with its own palette — the light one is dark-on-white, the dark
/// one is pastel-on-near-black — instead of reusing one 16-colour table. That is what makes
/// light mode readable: xterm's bright yellow/cyan on a white background is not.
enum TerminalPalette {
    /// `LIGHT_MODE_NORMAL_COLORS` (`default_themes.rs:32-41`).
    /// Yellow and cyan are darkened from harp's (`#A89C14`, `#20A5BA`) to clear 3:1 on white.
    private static let lightNormal: [UInt32] = [
        0x212121, 0xC30771, 0x10A778, 0x8A7F00, 0x008EC4, 0x523C79, 0x15828F, 0xE0E0E0,
    ]
    /// `LIGHT_MODE_BRIGHT_COLORS` (`default_themes.rs:42-51`).
    /// Bright yellow/cyan/green likewise (`#F3E430`, `#4FB8CC`, `#5FD7AF` were ~1.2–2:1).
    private static let lightBright: [UInt32] = [
        0x212121, 0xFB007A, 0x1E9E77, 0x9A8C00, 0x1A8FC4, 0x6855DE, 0x1F8A9A, 0xF1F1F1,
    ]
    /// `DARK_MODE_NORMAL_COLORS` (`default_themes.rs:11-20`).
    private static let darkNormal: [UInt32] = [
        0x616161, 0xFF8272, 0xB4FA72, 0xFEFDC2, 0xA5D5FE, 0xFF8FFD, 0xD0D1FE, 0xF1F1F1,
    ]
    /// `DARK_MODE_BRIGHT_COLORS` (`default_themes.rs:21-30`).
    private static let darkBright: [UInt32] = [
        0x8E8E8E, 0xFFC4BD, 0xD6FCB9, 0xFEFDD5, 0xC1E3FE, 0xFFB1FE, 0xE5E6FE, 0xFEFFFF,
    ]

    /// The `#rrggbb` value for ANSI index 0–15 in an appearance; `nil` for the 256-colour
    /// cube, which stays with the renderer's xterm table.
    static func hex(_ index: Int, appearance: NSAppearance) -> UInt32? {
        guard (0..<16).contains(index) else { return nil }
        let dark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let table = dark
            ? (index < 8 ? darkNormal : darkBright)
            : (index < 8 ? lightNormal : lightBright)
        return table[index % 8]
    }

    /// WCAG relative luminance: sRGB channels are linearised first, otherwise mid-greys look
    /// far darker than they are and a 4.5:1 floor would repaint readable text.
    private static func luminance(_ color: NSColor) -> CGFloat {
        let srgb = color.usingColorSpace(.sRGB) ?? color
        func linear(_ channel: CGFloat) -> CGFloat {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(srgb.redComponent)
            + 0.7152 * linear(srgb.greenComponent)
            + 0.0722 * linear(srgb.blueComponent)
    }

    private static func contrast(_ a: CGFloat, _ b: CGFloat) -> CGFloat {
        (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }

    /// Keeps text readable without destroying a theme's colours.
    ///
    /// Two different problems, two thresholds:
    /// * **Neutral** colours (greys, and "white"/"black" from a TUI's dark-canvas palette) carry
    ///   no hue to preserve, so they are lifted to a real 4.5:1 — that is what makes a shell
    ///   prompt painted with `SGR 37` legible on the light card.
    /// * **Coloured** text is the theme's identity; only colours below 1.2:1 (invisible) are
    ///   nudged, otherwise the hue survives untouched.
    static func readable(_ color: NSColor, on background: NSColor) -> NSColor {
        let fg = color.usingColorSpace(.sRGB) ?? color
        let bg = background.usingColorSpace(.sRGB) ?? background
        let spread = max(fg.redComponent, max(fg.greenComponent, fg.blueComponent))
            - min(fg.redComponent, min(fg.greenComponent, fg.blueComponent))
        let minimumRatio: CGFloat = spread < 0.12 ? 4.5 : 1.2
        let bgLum = luminance(bg)
        if contrast(luminance(fg), bgLum) >= minimumRatio { return fg }

        let extreme: NSColor = bgLum > 0.5 ? .black : .white
        var low: CGFloat = 0
        var high: CGFloat = 1
        var best = mix(fg, toward: extreme, by: 1)
        for _ in 0..<12 {
            let mid = (low + high) / 2
            let candidate = mix(fg, toward: extreme, by: mid)
            if contrast(luminance(candidate), bgLum) >= minimumRatio {
                best = candidate
                high = mid
            } else {
                low = mid
            }
        }
        return best
    }

    private static func mix(_ color: NSColor, toward target: NSColor, by amount: CGFloat) -> NSColor {
        let base = color.usingColorSpace(.sRGB) ?? color
        let end = target.usingColorSpace(.sRGB) ?? target
        return NSColor(
            srgbRed: base.redComponent * (1 - amount) + end.redComponent * amount,
            green: base.greenComponent * (1 - amount) + end.greenComponent * amount,
            blue: base.blueComponent * (1 - amount) + end.blueComponent * amount,
            alpha: base.alphaComponent
        )
    }

    /// The 16 ANSI values a client reports to the host (`appearance.ansi`).
    static func hexValues(appearance: NSAppearance) -> [UInt32] {
        (0..<16).compactMap { hex($0, appearance: appearance) }
    }

    /// `0xRRGGBB` of a dynamic token as the given appearance resolves it.
    static func hex(_ color: NSColor, appearance: NSAppearance) -> UInt32 {
        var value: UInt32 = 0
        appearance.performAsCurrentDrawingAppearance {
            let srgb = color.usingColorSpace(.sRGB) ?? color
            value = (UInt32((srgb.redComponent * 255).rounded()) << 16)
                | (UInt32((srgb.greenComponent * 255).rounded()) << 8)
                | UInt32((srgb.blueComponent * 255).rounded())
        }
        return value
    }

    static func color(_ index: Int, appearance: NSAppearance) -> NSColor? {
        hex(index, appearance: appearance).map { value in
            NSColor(
                srgbRed: CGFloat((value >> 16) & 0xff) / 255,
                green: CGFloat((value >> 8) & 0xff) / 255,
                blue: CGFloat(value & 0xff) / 255,
                alpha: 1
            )
        }
    }
}
