import AppKit

/// The colours one frame is drawn with, resolved on the main thread so the render thread
/// never touches `NSAppearance` (dynamic colours only resolve against the drawing appearance).
///
/// Cell colour values: the sentinel 0 takes the theme default,
/// `0x01000000 | idx` is a palette entry, anything else is `0xRRGGBB`.
struct CellPalette: Equatable, Sendable {
    /// ANSI 0–15 for the appearance.
    var ansi: [UInt32]
    var foreground: UInt32
    var background: UInt32

    static func resolve(for appearance: NSAppearance) -> CellPalette {
        CellPalette(
            ansi: TerminalPalette.hexValues(appearance: appearance),
            foreground: TerminalPalette.hex(SidebarTheme.terminalForeground, appearance: appearance),
            background: TerminalPalette.hex(SidebarTheme.terminalBackground, appearance: appearance)
        )
    }

    /// A cell's `(fg, bg)` as `0xRRGGBB`, reverse video applied.
    func colors(fg: UInt32, bg: UInt32, attrs: UInt8) -> (fg: UInt32, bg: UInt32) {
        var front = fg == 0 ? foreground : rgb(fg)
        var back = bg == 0 ? background : rgb(bg)
        if attrs & CellAttrs.reverse != 0 { swap(&front, &back) }
        return (front, back)
    }

    /// `0xRRGGBB` for a cell colour value; palette indices go through the ANSI table and
    /// xterm's 256-colour cube.
    func rgb(_ value: UInt32) -> UInt32 {
        guard value & 0xff00_0000 == 0x0100_0000 else { return value & 0x00ff_ffff }
        let index = Int(value & 0xff)
        if index < 16, index < ansi.count { return ansi[index] }
        if index < 232 {
            let cube = index - 16
            let steps: [UInt32] = [0, 95, 135, 175, 215, 255]
            return (steps[(cube / 36) % 6] << 16) | (steps[(cube / 6) % 6] << 8) | steps[cube % 6]
        }
        let level = UInt32(8 + (index - 232) * 10)
        return (level << 16) | (level << 8) | level
    }

    /// Text colour after the contrast floor (`TerminalPalette.readable`), so a TUI's greys
    /// stay legible on the light card in this renderer too.
    func readable(_ value: UInt32) -> UInt32 {
        let color = TerminalPalette.readable(Self.color(value), on: Self.color(background))
        let srgb = color.usingColorSpace(.sRGB) ?? color
        return (UInt32((srgb.redComponent * 255).rounded()) << 16)
            | (UInt32((srgb.greenComponent * 255).rounded()) << 8)
            | UInt32((srgb.blueComponent * 255).rounded())
    }

    static func color(_ value: UInt32) -> NSColor {
        NSColor(
            srgbRed: CGFloat((value >> 16) & 0xff) / 255,
            green: CGFloat((value >> 8) & 0xff) / 255,
            blue: CGFloat(value & 0xff) / 255,
            alpha: 1
        )
    }

    /// Straight-alpha RGBA in 0…1 for the GPU.
    static func components(_ value: UInt32, alpha: Float = 1) -> SIMD4<Float> {
        SIMD4(
            Float((value >> 16) & 0xff) / 255,
            Float((value >> 8) & 0xff) / 255,
            Float(value & 0xff) / 255,
            alpha
        )
    }
}
