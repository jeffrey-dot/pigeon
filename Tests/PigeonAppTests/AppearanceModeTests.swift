import AppKit
import XCTest
@testable import PigeonApp

/// Pinning an appearance overrides the system, so "follow the system" has to be reachable
/// from the toggle — otherwise the app can never go back to tracking macOS.
final class AppearanceModeTests: XCTestCase {
    func testTheToggleAlwaysCyclesBackToTheSystem() {
        XCTAssertEqual(AppearanceMode.system.next, .dark)
        XCTAssertEqual(AppearanceMode.dark.next, .light)
        XCTAssertEqual(AppearanceMode.light.next, .system)
        XCTAssertEqual(
            [AppearanceMode.system, .dark, .light, .system, .dark].map(\.rawValue),
            ["system", "dark", "light", "system", "dark"]
        )
    }

    func testOnlySystemLeavesTheAppearanceUnset() {
        XCTAssertNil(AppearanceMode.system.appearance)
        XCTAssertEqual(AppearanceMode.dark.appearance?.name, .darkAqua)
        XCTAssertEqual(AppearanceMode.light.appearance?.name, .aqua)
    }

    func testEnvironmentDefaultsToFollowingTheSystem() {
        XCTAssertEqual(AppearanceMode.fromEnvironment(nil), .system)
        XCTAssertEqual(AppearanceMode.fromEnvironment("system"), .system)
        XCTAssertEqual(AppearanceMode.fromEnvironment("Light"), .light)
        XCTAssertEqual(AppearanceMode.fromEnvironment("dark"), .dark)
    }
}
