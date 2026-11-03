import AppKit
import XCTest
@testable import PigeonApp
import PigeonCore

final class LayoutModelTests: XCTestCase {
    private func model() -> LayoutModel {
        // Left/right split at 0.5, right half split top/bottom at 0.4.
        LayoutModel(
            panes: [
                .init(id: 1, rect: CGRect(x: 0, y: 0, width: 0.5, height: 1)),
                .init(id: 2, rect: CGRect(x: 0.5, y: 0, width: 0.5, height: 0.4)),
                .init(id: 3, rect: CGRect(x: 0.5, y: 0.4, width: 0.5, height: 0.6)),
            ],
            dividers: [
                .init(splitID: 1, axis: PG_AXIS_VERTICAL, ratio: 0.5,
                      box: CGRect(x: 0, y: 0, width: 1, height: 1)),
                .init(splitID: 2, axis: PG_AXIS_HORIZONTAL, ratio: 0.4,
                      box: CGRect(x: 0.5, y: 0, width: 0.5, height: 1)),
            ],
            focus: 1
        )
    }

    func testDividerHitTestUsesItsOwnAxis() {
        let model = model()
        XCTAssertEqual(model.divider(at: CGPoint(x: 0.5, y: 0.5), tolerance: 0.01)?.splitID, 1)
        XCTAssertEqual(model.divider(at: CGPoint(x: 0.75, y: 0.4), tolerance: 0.01)?.splitID, 2)
        XCTAssertNil(model.divider(at: CGPoint(x: 0.75, y: 0.75), tolerance: 0.01))
        // A vertical divider only reacts inside its box.
        XCTAssertNil(model.divider(at: CGPoint(x: 0.5, y: 1.5), tolerance: 0.01))
    }

    func testPaneHitTestAndFocusCycle() {
        let model = model()
        XCTAssertEqual(model.pane(at: CGPoint(x: 0.2, y: 0.5))?.id, 1)
        XCTAssertEqual(model.pane(at: CGPoint(x: 0.8, y: 0.9))?.id, 3)
        XCTAssertEqual(model.orderedPanes, [1, 2, 3], "tree order")
        XCTAssertEqual(model.nextFocus(forward: true), 2)
        var focused = model
        focused.focus = 3
        XCTAssertEqual(focused.nextFocus(forward: true), 1, "wraps")
        XCTAssertEqual(focused.nextFocus(forward: false), 2, "wraps backwards")
    }

    func testProposedRatioIsASimpleCoordinateMapping() {
        let model = model()
        let vertical = model.dividers[0]
        XCTAssertEqual(model.proposedRatio(vertical, at: CGPoint(x: 0.7, y: 0.5)), 0.7, accuracy: 1e-6)
        let horizontal = model.dividers[1]
        XCTAssertEqual(model.proposedRatio(horizontal, at: CGPoint(x: 0.8, y: 0.5)), 0.5, accuracy: 1e-6)
    }

    func testEmptyModelHasNoFocus() {
        XCTAssertNil(LayoutModel.empty.nextFocus(forward: true))
        XCTAssertNil(LayoutModel.empty.pane(at: .zero))
    }
}
