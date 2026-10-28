import AppKit
import XCTest
@testable import PigeonApp

/// The terminal palette is copied from harp's per-appearance ANSI tables
/// (`app/src/themes/default_themes.rs:11-51`). Light mode is dark-on-white on purpose: reusing
/// xterm's bright yellow/cyan on a white background is what made light mode unreadable.
final class TerminalPaletteTests: XCTestCase {
    func testLightPaletteMatchesHarp() {
        let light = NSAppearance(named: .aqua)!
        XCTAssertEqual(TerminalPalette.hex(0, appearance: light), 0x212121)
        XCTAssertEqual(TerminalPalette.hex(1, appearance: light), 0xC30771)
        XCTAssertEqual(TerminalPalette.hex(3, appearance: light), 0xA89C14)
        XCTAssertEqual(TerminalPalette.hex(9, appearance: light), 0xFB007A) // bright red
        XCTAssertEqual(TerminalPalette.hex(13, appearance: light), 0x6855DE) // bright magenta
    }

    func testDarkPaletteMatchesHarp() {
        let dark = NSAppearance(named: .darkAqua)!
        XCTAssertEqual(TerminalPalette.hex(0, appearance: dark), 0x616161)
        XCTAssertEqual(TerminalPalette.hex(1, appearance: dark), 0xFF8272)
        XCTAssertEqual(TerminalPalette.hex(3, appearance: dark), 0xFEFDC2)
        XCTAssertEqual(TerminalPalette.hex(8, appearance: dark), 0x8E8E8E)
        XCTAssertEqual(TerminalPalette.hex(15, appearance: dark), 0xFEFFFF)
    }

    func testCubeIndicesStayWithTheRenderer() {
        let light = NSAppearance(named: .aqua)!
        XCTAssertNil(TerminalPalette.hex(16, appearance: light))
        XCTAssertNil(TerminalPalette.hex(231, appearance: light))
    }

    /// The protocol is the fix now, so the floor only stops *invisible* text: a TUI's own
    /// light-theme colours (which it picked after asking us for the background) pass through
    /// untouched, otherwise every theme collapses to one muddy colour.
    func testTheFloorOnlyRescuesInvisibleText() {
        let white = NSColor.white
        let washed = NSColor(srgbRed: 0xe6 / 255, green: 0xe6 / 255, blue: 0xe6 / 255, alpha: 1)
        // Neutral text gets body-text contrast: a shell prompt painted `SGR 37` (`#E0E0E0` in
        // harp's light palette) must become readable on the white card.
        let lifted = TerminalPalette.readable(washed, on: white)
        XCTAssertGreaterThanOrEqual(contrast(lifted, white), 4.4, "neutral text must clear 4.5:1")
        let ansiWhite = NSColor(srgbRed: 0xE0 / 255, green: 0xE0 / 255, blue: 0xE0 / 255, alpha: 1)
        XCTAssertGreaterThanOrEqual(contrast(TerminalPalette.readable(ansiWhite, on: white), white), 4.4)

        let nearBlack = NSColor(srgbRed: 0x05 / 255, green: 0x05 / 255, blue: 0x05 / 255, alpha: 1)
        let dim = NSColor(srgbRed: 0x0a / 255, green: 0x0a / 255, blue: 0x0a / 255, alpha: 1)
        XCTAssertGreaterThanOrEqual(contrast(TerminalPalette.readable(dim, on: nearBlack), nearBlack), 1.19)

        // A theme's own colours are left alone: harp's bright yellow is ~1.25:1 on white and
        // still recognisably yellow, so it must not be swapped for a grey.
        let yellow = NSColor(srgbRed: 0xA8 / 255, green: 0x9C / 255, blue: 0x14 / 255, alpha: 1)
        XCTAssertEqual(TerminalPalette.readable(yellow, on: white), yellow.usingColorSpace(.sRGB))

        // Already past 4.5:1, so it must pass through untouched.
        let midGrey = NSColor(srgbRed: 0x6b / 255, green: 0x6b / 255, blue: 0x6b / 255, alpha: 1)
        XCTAssertEqual(TerminalPalette.readable(midGrey, on: white), midGrey.usingColorSpace(.sRGB))
    }

    private func contrast(_ a: NSColor, _ b: NSColor) -> CGFloat {
        let (la, lb) = (luminance(a), luminance(b))
        return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
    }

    private func luminance(_ color: NSColor) -> CGFloat {
        let srgb = color.usingColorSpace(.sRGB) ?? color
        func linear(_ channel: CGFloat) -> CGFloat {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(srgb.redComponent)
            + 0.7152 * linear(srgb.greenComponent)
            + 0.0722 * linear(srgb.blueComponent)
    }

    /// The host answers `OSC 4/10/11` from these values, so the message must carry the same
    /// 16 colours the renderer draws with.
    func testHexValuesAreTheSixteenPaletteEntriesPerAppearance() {
        let light = TerminalPalette.hexValues(appearance: NSAppearance(named: .aqua)!)
        let dark = TerminalPalette.hexValues(appearance: NSAppearance(named: .darkAqua)!)
        XCTAssertEqual(light.count, 16)
        XCTAssertEqual(dark.count, 16)
        XCTAssertEqual(light[0], 0x212121)
        XCTAssertEqual(light[9], 0xFB007A)
        XCTAssertEqual(dark[0], 0x616161)
        XCTAssertEqual(dark[15], 0xFEFFFF)
        XCTAssertNotEqual(light, dark)
    }

    func testForegroundAndBackgroundHexFollowTheAppearance() {
        let light = NSAppearance(named: .aqua)!
        let dark = NSAppearance(named: .darkAqua)!
        XCTAssertEqual(TerminalPalette.hex(SidebarTheme.terminalBackground, appearance: light), 0xFFFFFF)
        XCTAssertEqual(TerminalPalette.hex(SidebarTheme.terminalForeground, appearance: light), 0x111111)
        XCTAssertEqual(TerminalPalette.hex(SidebarTheme.terminalBackground, appearance: dark), 0x050505)
        XCTAssertEqual(TerminalPalette.hex(SidebarTheme.terminalForeground, appearance: dark), 0xFFFFFF)
    }

    func testCellHeightUsesHarpLineHeightRatio() {
        // 13pt × 1.2 = 15.6; the cell width still comes from the "M" advance.
        XCTAssertEqual(CoreTextRenderer.cellSize().height, 15.6, accuracy: 0.01)
    }
}
