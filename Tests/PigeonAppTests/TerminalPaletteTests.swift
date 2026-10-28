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

    /// TUIs pick greys for a dark canvas; on the light card those land at `#e6e6e6`. The floor
    /// must push them to a readable contrast without touching colours that already read.
    func testContrastFloorLiftsDarkCanvasGreys() {
        let white = NSColor.white
        let washed = NSColor(srgbRed: 0xe6 / 255, green: 0xe6 / 255, blue: 0xe6 / 255, alpha: 1)
        let lifted = TerminalPalette.readable(washed, on: white)
        XCTAssertGreaterThanOrEqual(contrast(lifted, white), 3.0, "washed-out grey must clear 3:1 on white")

        let nearBlack = NSColor(srgbRed: 0x05 / 255, green: 0x05 / 255, blue: 0x05 / 255, alpha: 1)
        let dim = NSColor(srgbRed: 0x22 / 255, green: 0x22 / 255, blue: 0x22 / 255, alpha: 1)
        let brightened = TerminalPalette.readable(dim, on: nearBlack)
        XCTAssertGreaterThanOrEqual(contrast(brightened, nearBlack), 3.0, "dim grey must clear 3:1 on near-black")

        let ink = NSColor(srgbRed: 0x11 / 255, green: 0x11 / 255, blue: 0x11 / 255, alpha: 1)
        XCTAssertEqual(TerminalPalette.readable(ink, on: white), ink.usingColorSpace(.sRGB))

        // Saturated colours keep their identity: only the *minimum* correction is applied, so
        // darkening yellow and blue must not land on the same grey.
        let yellow = NSColor(srgbRed: 0xF3 / 255, green: 0xE4 / 255, blue: 0x30 / 255, alpha: 1)
        let blue = NSColor(srgbRed: 0x20 / 255, green: 0xBB / 255, blue: 0xFC / 255, alpha: 1)
        let fixedYellow = TerminalPalette.readable(yellow, on: white).usingColorSpace(.sRGB)!
        let fixedBlue = TerminalPalette.readable(blue, on: white).usingColorSpace(.sRGB)!
        XCTAssertGreaterThan(fixedYellow.redComponent - fixedYellow.blueComponent, 0.15, "yellow must stay warm")
        XCTAssertGreaterThan(fixedBlue.blueComponent - fixedBlue.redComponent, 0.1, "blue must stay cool")
    }

    private func contrast(_ a: NSColor, _ b: NSColor) -> CGFloat {
        let (la, lb) = (luminance(a), luminance(b))
        return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
    }

    private func luminance(_ color: NSColor) -> CGFloat {
        let srgb = color.usingColorSpace(.sRGB) ?? color
        return 0.2126 * srgb.redComponent + 0.7152 * srgb.greenComponent + 0.0722 * srgb.blueComponent
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
