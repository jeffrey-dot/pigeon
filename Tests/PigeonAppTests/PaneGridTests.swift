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
}
