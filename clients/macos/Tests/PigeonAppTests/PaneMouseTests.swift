import AppKit
import PigeonCore
import XCTest
@testable import PigeonApp

/// Where the wheel goes on the alternate screen, through the pane itself.
final class PaneMouseTests: XCTestCase {
    private func altPane(modes: UInt32) -> (PaneView, () -> [[UInt8]]) {
        let view = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var blank = [PgCell](
            repeating: PgCell(size: UInt32(MemoryLayout<PgCell>.size), codepoint: 32, fg: 0, bg: 0,
                              attrs: 0, hyperlink_id: 0, width: 1),
            count: 4)
        blank.withUnsafeMutableBufferPointer { cells in
            var rows = [PgRow(size: UInt32(MemoryLayout<PgRow>.size), cells: UnsafePointer(cells.baseAddress!))]
            rows.withUnsafeBufferPointer { rowBuffer in
                var snapshot = PgSnapshot(
                    size: UInt32(MemoryLayout<PgSnapshot>.size), version: 1, pane_id: 1,
                    cols: 4, rows: 1, alt_screen: 1,
                    cursor: PgCursor(size: UInt32(MemoryLayout<PgCursor>.size), x: 0, y: 0, visible: 1, shape: 0),
                    scroll_region: PgScrollRegion(size: UInt32(MemoryLayout<PgScrollRegion>.size), top: 0, bottom: 0),
                    screen: rowBuffer.baseAddress, history: nil, history_count: 0)
                view.apply(snapshot: &snapshot)
            }
        }
        view.modes = modes
        var sent: [[UInt8]] = []
        view.onInput = { sent.append($0) }
        return (view, { sent })
    }

    private func wheel() -> NSEvent {
        // `wheelOnAlternateScreen` only reads the position and modifiers from the event.
        NSEvent.mouseEvent(
            with: .leftMouseDown, location: NSPoint(x: 1, y: 1), modifierFlags: [], timestamp: 0,
            windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 0)!
    }

    func testLessGetsArrowKeysFromTheWheel() {
        let (view, sent) = altPane(modes: TerminalModes.alternateScroll)
        XCTAssertTrue(view.wheelOnAlternateScreen(wheel(), lines: 2))
        XCTAssertEqual(sent(), [Array("\u{1b}[A".utf8), Array("\u{1b}[A".utf8)])
    }

    func testAMouseAwareProgramGetsWheelReports() {
        let (view, sent) = altPane(modes: TerminalModes.mouseClick | TerminalModes.mouseSGR)
        XCTAssertTrue(view.wheelOnAlternateScreen(wheel(), lines: -1))
        XCTAssertEqual(sent().count, 1)
        XCTAssertTrue(String(decoding: sent()[0], as: UTF8.self).hasPrefix("\u{1b}[<65;"))
    }

    func testWithNeitherModeTheWheelDoesNothing() {
        let (view, sent) = altPane(modes: 0)
        XCTAssertFalse(view.wheelOnAlternateScreen(wheel(), lines: 3))
        XCTAssertTrue(sent().isEmpty)
    }
}
