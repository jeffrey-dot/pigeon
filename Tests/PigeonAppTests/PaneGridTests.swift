import CoreGraphics
import XCTest
@testable import PigeonApp

/// A pane must never tell the host a grid it cannot show: a zero-sized view used to report
/// 2×1, the host resized the terminal to 2×1 and dropped the scrollback, and an idle TUI
/// never repainted (the blank `π - core` pane).
final class PaneGridTests: XCTestCase {
    private let cell = CGSize(width: 10, height: 18)

    func testUnlaidOutViewReportsNothing() {
        XCTAssertNil(PaneView.gridSize(for: .zero, cellSize: cell))
        XCTAssertNil(PaneView.gridSize(for: CGSize(width: 10, height: 18), cellSize: cell), "1 column is degenerate")
        XCTAssertNil(PaneView.gridSize(for: CGSize(width: 300, height: 10), cellSize: cell), "half a row is degenerate")
        XCTAssertNil(PaneView.gridSize(for: CGSize(width: 300, height: 100), cellSize: .zero))
    }

    func testNormalBoundsReportTheGridTheCellsFit() {
        let grid = PaneView.gridSize(for: CGSize(width: 970, height: 630), cellSize: cell)
        XCTAssertEqual(grid?.cols, 97)
        XCTAssertEqual(grid?.rows, 35)
        // Remainders are dropped, never rounded up: the last partial cell does not exist.
        XCTAssertEqual(PaneView.gridSize(for: CGSize(width: 979, height: 647), cellSize: cell)?.cols, 97)
        XCTAssertEqual(PaneView.gridSize(for: CGSize(width: 979, height: 647), cellSize: cell)?.rows, 35)
    }

    /// The pane no longer keeps history: it asks the host to look back, so the only rule
    /// left here is that an upward wheel becomes a "look back" request of the right size.
    func testWheelDirectionBecomesAScrollRequest() {
        // A line-based wheel already reports lines; a precise (trackpad) one reports pixels.
        let up = PaneView.wheelLines(deltaY: 1, precise: false, cellHeight: 18, remainder: 0)
        XCTAssertEqual(up.lines, 1, "wheel up moves one line into history")
        let down = PaneView.wheelLines(deltaY: -3, precise: false, cellHeight: 18, remainder: 0)
        XCTAssertEqual(down.lines, -3, "wheel down moves forward")
    }

    func testTrackpadPixelsAccumulateIntoWholeLines() {
        // Two 6pt events with 18pt rows move one line, not zero.
        let first = PaneView.wheelLines(deltaY: 12, precise: true, cellHeight: 18, remainder: 0)
        XCTAssertEqual(first.lines, 0)
        XCTAssertEqual(first.remainder, 12)
        let second = PaneView.wheelLines(deltaY: 12, precise: true, cellHeight: 18, remainder: first.remainder)
        XCTAssertEqual(second.lines, 1)
        XCTAssertEqual(second.remainder, 6)
        // A line-based wheel reports lines directly and keeps no remainder.
        let wheel = PaneView.wheelLines(deltaY: -3, precise: false, cellHeight: 18, remainder: 12)
        XCTAssertEqual(wheel.lines, -3)
        XCTAssertEqual(wheel.remainder, 0)
    }
}
