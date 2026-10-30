import AppKit
import XCTest
@testable import PigeonApp

/// Proves the "switch appearance without a restart" requirement at the view level: one
/// view instance, rendered twice under two appearances, must paint the two harp surfaces.
final class SidebarRenderingTests: XCTestCase {
    private func pixel(of view: NSView, appearance name: NSAppearance.Name) -> (Int, Int, Int) {
        view.appearance = NSAppearance(named: name)
        let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)!
        view.cacheDisplay(in: view.bounds, to: rep)
        let color = rep.colorAt(x: 4, y: 4)!.usingColorSpace(.sRGB)!
        return (
            Int((color.redComponent * 255).rounded()),
            Int((color.greenComponent * 255).rounded()),
            Int((color.blueComponent * 255).rounded())
        )
    }

    /// The unread dot used to be a `CALayer` color, which freezes the palette at the moment
    /// of assignment; drawing it keeps the dot in step with the appearance.
    func testTheUnreadDotFollowsTheAppearanceOnTheSameView() {
        let view = SidebarDotView(frame: NSRect(x: 0, y: 0, width: 6, height: 6))
        let light = pixel(of: view, appearance: .aqua)
        let dark = pixel(of: view, appearance: .darkAqua)
        // Colour-managed bitmaps shift a channel by one on some machines; the exact values
        // are pinned in SidebarThemeTests.
        XCTAssertEqual(light.0, 0x3a, accuracy: 2) // ICON_STRONG light
        XCTAssertEqual(light.2, 0x3c, accuracy: 2)
        XCTAssertEqual(dark.0, 0xe5, accuracy: 2) // ICON_STRONG dark
        XCTAssertEqual(dark.2, 0xea, accuracy: 2)
    }

    /// The card is painted with the theme background (harp `view.rs:22241`), so under a
    /// light appearance it must be white, not the old fixed dark surface.
    func testTheCardSurfaceFollowsTheAppearance() {
        let card = CardView(frame: NSRect(x: 0, y: 0, width: 80, height: 60))
        func rgb(_ appearance: NSAppearance.Name) -> (Int, Int, Int) {
            card.appearance = NSAppearance(named: appearance)
            card.viewDidChangeEffectiveAppearance()
            let color = card.surfaceColorForTesting!
            return (
                Int((color.redComponent * 255).rounded()),
                Int((color.greenComponent * 255).rounded()),
                Int((color.blueComponent * 255).rounded())
            )
        }
        XCTAssertEqual(rgb(.aqua).0, 0xff) // theme background light
        XCTAssertEqual(rgb(.aqua).2, 0xff)
        XCTAssertEqual(rgb(.darkAqua).0, 0x05) // theme background dark
        XCTAssertEqual(rgb(.darkAqua).2, 0x05)
    }

    /// Cells without an explicit colour take the theme's foreground/background, resolved for
    /// the appearance that is drawing (harp `default_themes.rs:262/275`).
    func testTerminalDefaultsFollowTheAppearance() {
        let light = CellPalette.resolve(for: NSAppearance(named: .aqua)!)
        let dark = CellPalette.resolve(for: NSAppearance(named: .darkAqua)!)
        XCTAssertEqual(light.foreground, 0x0011_1111) // #111111
        XCTAssertEqual(light.background, 0x00ff_ffff) // #ffffff
        XCTAssertEqual(dark.foreground, 0x00ff_ffff) // #ffffff
        XCTAssertEqual(dark.background, 0x0005_0505) // #050505
    }

    func testTheSidebarSurfaceFollowsTheAppearanceOnTheSameView() {
        let view = SidebarBackgroundView(frame: NSRect(x: 0, y: 0, width: 120, height: 80))
        let light = pixel(of: view, appearance: .aqua)
        let dark = pixel(of: view, appearance: .darkAqua)
        // Bitmap round-tripping can shift a channel by one; the exact values are pinned
        // in SidebarThemeTests, here we only care that the two branches are both live.
        XCTAssertEqual(light.0, 0xe8, accuracy: 2) // SIDEBAR light
        XCTAssertEqual(light.2, 0xeb, accuracy: 2)
        XCTAssertEqual(dark.0, 0x23, accuracy: 2) // SIDEBAR dark
        XCTAssertEqual(dark.2, 0x26, accuracy: 2)
    }
}
