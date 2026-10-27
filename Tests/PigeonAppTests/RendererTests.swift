import AppKit
import XCTest
@testable import PigeonApp

/// Cell placement, without a window server: what matters is that every run is
/// pinned to its own column, so a wide character cannot shift the rest of the row.
final class RendererTests: XCTestCase {
    private func cell(_ character: Character, width: UInt8, fg: UInt32 = 0x00e5e5e5) -> PaneView.Cell {
        PaneView.Cell(
            codepoint: character.unicodeScalars.first?.value ?? 32,
            attrs: 0,
            fg: fg,
            bg: 0,
            width: width
        )
    }

    func testWideCharOwnsTwoColumns() {
        let row = [
            cell("A", width: 1),
            cell("中", width: 2),
            cell(" ", width: 0), // continuation
            cell("B", width: 1),
        ]
        let runs = CoreTextRenderer.textRuns(for: row)
        XCTAssertEqual(runs.map(\.startColumn), [0, 1, 3])
        XCTAssertEqual(runs.map(\.text), ["A", "中", "B"])
    }

    func testSameColourCellsStayInOneRunAtTheirStartColumn() {
        let row = [
            cell("a", width: 1),
            cell("b", width: 1),
            cell("c", width: 1, fg: 0x00ff0000),
            cell("d", width: 1, fg: 0x00ff0000),
        ]
        let runs = CoreTextRenderer.textRuns(for: row)
        XCTAssertEqual(runs.map(\.startColumn), [0, 2])
        XCTAssertEqual(runs.map(\.text), ["ab", "cd"])
    }

    func testWideCharAfterWideCharKeepsTheGrid() {
        let row = [
            cell("中", width: 2),
            cell(" ", width: 0),
            cell("文", width: 2),
            cell(" ", width: 0),
            cell("x", width: 1),
        ]
        let runs = CoreTextRenderer.textRuns(for: row)
        XCTAssertEqual(runs.map(\.startColumn), [0, 2, 4])
    }
}
