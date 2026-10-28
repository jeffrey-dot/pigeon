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
