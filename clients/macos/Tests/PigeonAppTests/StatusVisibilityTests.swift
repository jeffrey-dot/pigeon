import XCTest
@testable import PigeonApp

final class StatusVisibilityTests: XCTestCase {
    private var visible: StatusVisibility {
        StatusVisibility(activeHost: "local", displayedHost: "local", selectedSession: 1,
                         displayedSession: 1, ready: true, windowFocused: true,
                         windowVisible: true, presentedPanes: [10])
    }

    func testOnlyPresentedPanesOfTheFocusedSessionCanBeSeen() {
        XCTAssertEqual(visible.panes(for: "local", session: 1), [10])
        XCTAssertEqual(visible.panes(for: "devbox", session: 1), [])
        XCTAssertEqual(visible.panes(for: "local", session: 2), [])
        XCTAssertEqual(visible.panes(for: "local", session: nil), [])
    }

    func testInactiveOccludedDisconnectedAndTransitionalViewsAreNotSeen() {
        var view = visible
        view.windowFocused = false
        XCTAssertTrue(view.panes(for: "local", session: 1).isEmpty)
        view = visible
        view.windowVisible = false
        XCTAssertTrue(view.panes(for: "local", session: 1).isEmpty)
        view = visible
        view.ready = false
        XCTAssertTrue(view.panes(for: "local", session: 1).isEmpty)
        view = visible
        view.displayedHost = "devbox"
        XCTAssertTrue(view.panes(for: "local", session: 1).isEmpty)
        view = visible
        view.displayedSession = 2
        XCTAssertTrue(view.panes(for: "local", session: 1).isEmpty)
        view = visible
        view.presentedPanes = []
        XCTAssertTrue(view.panes(for: "local", session: 1).isEmpty)
    }
}
