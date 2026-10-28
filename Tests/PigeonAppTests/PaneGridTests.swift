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

    func testScrollingIsClampedToTheHistoryThePaneHolds() {
        XCTAssertEqual(PaneView.clampedOffset(-5, historyLines: 100), 0, "wheel down stops at the live grid")
        XCTAssertEqual(PaneView.clampedOffset(20, historyLines: 100), 20)
        XCTAssertEqual(PaneView.clampedOffset(500, historyLines: 100), 100, "no scrolling past the oldest line")
        XCTAssertEqual(PaneView.clampedOffset(5, historyLines: 0), 0)
    }

    func testTheAlternateScreenNeverScrolls() {
        XCTAssertFalse(PaneView.canScroll(alt: true, historyLines: 500), "vim/htop own the screen")
        XCTAssertFalse(PaneView.canScroll(alt: false, historyLines: 0), "no history, nothing to show")
        XCTAssertTrue(PaneView.canScroll(alt: false, historyLines: 500))
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
