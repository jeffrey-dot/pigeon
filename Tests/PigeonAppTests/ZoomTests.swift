import AppKit
import PigeonCore
import XCTest
@testable import PigeonApp

/// The client half of zoom (`M2`): what the shortcut asks for, which panes stay visible, and
/// that the menu item really carries ⌘⇧↩. The host half is covered in `core/tests/host.rs`.
final class ZoomTests: XCTestCase {
    func testTheShortcutAsksToZoomThenToUnzoom() {
        XCTAssertNil(
            SplitContainerView.zoomRequest(currentZoom: 0, focused: 0),
            "no focused pane, nothing to zoom")
        XCTAssertEqual(
            SplitContainerView.zoomRequest(currentZoom: 0, focused: 2), PG_MSG_ZOOM_PANE)
        XCTAssertEqual(
            SplitContainerView.zoomRequest(currentZoom: 3, focused: 2), PG_MSG_ZOOM_PANE,
            "zooming another pane switches the zoom")
        XCTAssertEqual(
            SplitContainerView.zoomRequest(currentZoom: 2, focused: 2), PG_MSG_UNZOOM,
            "pressing it again on the zoomed pane unzooms")
    }

    func testOnlyTheZoomedPaneStaysVisible() {
        XCTAssertFalse(SplitContainerView.hides(pane: 1, zoom: 0))
        XCTAssertFalse(SplitContainerView.hides(pane: 1, zoom: 1))
        XCTAssertTrue(SplitContainerView.hides(pane: 1, zoom: 2))
    }

    func testTheMenuCarriesCommandShiftReturn() {
        let item = SplitContainerView.zoomMenuItem()
        XCTAssertEqual(item.title, "Zoom Pane")
        XCTAssertEqual(item.keyEquivalent, "\r", "Return, not D (that one splits)")
        XCTAssertTrue(item.keyEquivalentModifierMask.contains(.command))
        XCTAssertTrue(item.keyEquivalentModifierMask.contains(.shift))
        XCTAssertEqual(item.action, #selector(SplitContainerView.toggleZoom(_:)))
        // The selector must actually exist on the view that owns it.
        XCTAssertTrue(SplitContainerView.instancesRespond(to: item.action!))
    }

    func testALayoutWithoutZoomShowsEveryPane() {
        let model = LayoutModel(
            panes: [
                LayoutModel.Pane(id: 1, rect: CGRect(x: 0, y: 0, width: 0.5, height: 1)),
                LayoutModel.Pane(id: 2, rect: CGRect(x: 0.5, y: 0, width: 0.5, height: 1)),
            ],
            dividers: [],
            focus: 1)
        XCTAssertEqual(model.zoom, 0)
        XCTAssertFalse(model.panes.contains { SplitContainerView.hides(pane: $0.id, zoom: model.zoom) })
    }
}
