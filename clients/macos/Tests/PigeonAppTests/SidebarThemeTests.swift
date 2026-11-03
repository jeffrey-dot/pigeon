import AppKit
import XCTest
@testable import PigeonApp

/// The palette in `docs/ui.md` §7.2 is copied from harp; these tests pin the two
/// branches so a stray `systemOrange` cannot creep back in.
final class SidebarThemeTests: XCTestCase {
    private func resolved(_ color: NSColor, dark: Bool) -> (r: Int, g: Int, b: Int) {
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
        var value = color
        appearance.performAsCurrentDrawingAppearance {
            value = color.usingColorSpace(.sRGB) ?? color
        }
        return (
            Int((value.redComponent * 255).rounded()),
            Int((value.greenComponent * 255).rounded()),
            Int((value.blueComponent * 255).rounded())
        )
    }

    func testTokensMatchTheDesignedLightAndDarkValues() {
        let cases: [(String, NSColor, Int, Int, Int, Int, Int, Int)] = [
            ("ink", SidebarTheme.ink, 0x1d, 0x1d, 0x1f, 0xe6, 0xe6, 0xe6),
            ("gray", SidebarTheme.gray, 0x6e, 0x6e, 0x73, 0x8e, 0x8e, 0x93),
            ("sectionName", SidebarTheme.sectionName, 0x7c, 0x7c, 0x82, 0x8e, 0x8e, 0x93),
            ("needsText", SidebarTheme.needsText, 0x9a, 0x57, 0x00, 0xf0, 0xb3, 0x5c),
            ("iconStrong", SidebarTheme.iconStrong, 0x3a, 0x3a, 0x3c, 0xe5, 0xe5, 0xea),
            ("sidebar", SidebarTheme.sidebar, 0xe8, 0xe8, 0xeb, 0x16, 0x17, 0x1a),
            ("selected", SidebarTheme.selected, 0xff, 0xff, 0xff, 0x2a, 0x2b, 0x2f),
            ("terminalBackground", SidebarTheme.terminalBackground, 0xff, 0xff, 0xff, 0x0e, 0x0f, 0x11),
            ("terminalForeground", SidebarTheme.terminalForeground, 0x11, 0x11, 0x11, 0xe6, 0xe6, 0xe6),
            ("divider", SidebarTheme.divider, 0xec, 0xec, 0xef, 0x24, 0x26, 0x2a),
        ]
        for (name, color, lr, lg, lb, dr, dg, db) in cases {
            let light = resolved(color, dark: false)
            let dark = resolved(color, dark: true)
            XCTAssertEqual([light.r, light.g, light.b], [lr, lg, lb], "\(name) light")
            XCTAssertEqual([dark.r, dark.g, dark.b], [dr, dg, db], "\(name) dark")
        }
    }

    func testOverlaysKeepHarpAlpha() {
        func alpha(_ color: NSColor, dark: Bool) -> Int {
            let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
            var value = color
            appearance.performAsCurrentDrawingAppearance {
                value = color.usingColorSpace(.sRGB) ?? color
            }
            return Int((value.alphaComponent * 255).rounded())
        }
        XCTAssertEqual(alpha(SidebarTheme.hover, dark: false), 13) // black 5%
        XCTAssertEqual(alpha(SidebarTheme.hover, dark: true), 15) // white 6%
        XCTAssertEqual(alpha(SidebarTheme.selectedEdge, dark: false), 20)
        XCTAssertEqual(alpha(SidebarTheme.selectedEdge, dark: true), 26)
    }

    func testNeedsLabelUsesHarpPluralization() {
        XCTAssertEqual(SidebarTheme.needsLabel(1), "1 needs you")
        XCTAssertEqual(SidebarTheme.needsLabel(2), "2 need you")
    }

    func testMetricsCarryHarpGeometry() {
        XCTAssertEqual(SidebarMetrics.width, 272)
        XCTAssertEqual(SidebarMetrics.rowHeight, 30)
        XCTAssertEqual(SidebarMetrics.rowRadius, 8)
        XCTAssertEqual(SidebarMetrics.sectionHeaderHeight, 28)
        XCTAssertEqual(SidebarMetrics.headerHeight, 56)
        XCTAssertEqual(SidebarMetrics.cardHeaderHeight, 40)
        XCTAssertEqual(SidebarMetrics.rowTitleSize, 13)
        XCTAssertEqual(SidebarMetrics.sectionTitleSize, 11.5)
    }
}
