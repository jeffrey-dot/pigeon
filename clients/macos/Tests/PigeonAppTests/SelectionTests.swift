import XCTest
@testable import PigeonApp

/// What a drag, double-click or triple-click selects, and the text ⌘C copies.
final class SelectionTests: XCTestCase {
    /// Rows of the same width; `中` takes two cells (the second a continuation).
    private func grid(_ lines: [String], cols: Int) -> [PaneView.Cell] {
        var cells: [PaneView.Cell] = []
        for line in lines {
            var row: [PaneView.Cell] = []
            for scalar in line.unicodeScalars {
                let wide = scalar.value >= 0x4E00 && scalar.value <= 0x9FFF
                row.append(PaneView.Cell(codepoint: scalar.value, attrs: 0, fg: 0, bg: 0, width: wide ? 2 : 1))
                if wide { row.append(PaneView.Cell(codepoint: 32, attrs: 0, fg: 0, bg: 0, width: 0)) }
            }
            while row.count < cols { row.append(.blank) }
            cells.append(contentsOf: row.prefix(cols))
        }
        return cells
    }

    private func point(_ row: Int, _ column: Int) -> TerminalSelection.Point {
        .init(row: row, column: column)
    }

    func testADragAcrossRowsJoinsThemAndDropsThePadding() {
        let cells = grid(["hello", "world"], cols: 8)
        var selection = TerminalSelection(at: point(0, 1))
        selection.head = point(1, 2)
        XCTAssertEqual(selection.text(cells: cells, cols: 8, rows: 2), "ello\nwor")
    }

    func testDraggingBackwardsSelectsTheSameText() {
        let cells = grid(["hello", "world"], cols: 8)
        var selection = TerminalSelection(at: point(1, 2))
        selection.head = point(0, 1)
        XCTAssertEqual(selection.text(cells: cells, cols: 8, rows: 2), "ello\nwor")
    }

    func testADoubleClickTakesAPathOrURLWhole() {
        let cells = grid(["see src/main.rs:12 now", "go https://a.io/x?y=1 ok"], cols: 26)
        let path = TerminalSelection(at: point(0, 8), unit: .word)
        XCTAssertEqual(path.text(cells: cells, cols: 26, rows: 2), "src/main.rs:12")
        let url = TerminalSelection(at: point(1, 5), unit: .word)
        XCTAssertEqual(url.text(cells: cells, cols: 26, rows: 2), "https://a.io/x?y=1")
        let space = TerminalSelection(at: point(0, 3), unit: .word)
        XCTAssertEqual(space.text(cells: cells, cols: 26, rows: 2), "", "a blank is not a word")
    }

    func testATripleClickTakesTheLine() {
        let cells = grid(["$ cargo test", "ok"], cols: 16)
        let line = TerminalSelection(at: point(0, 4), unit: .line)
        XCTAssertEqual(line.text(cells: cells, cols: 16, rows: 2), "$ cargo test")
    }

    func testWideCharactersCopyOnceEach() {
        let cells = grid(["中文 ok"], cols: 10)
        var selection = TerminalSelection(at: point(0, 0))
        selection.head = point(0, 6)
        XCTAssertEqual(selection.text(cells: cells, cols: 10, rows: 1), "中文 ok")
        let word = TerminalSelection(at: point(0, 1), unit: .word)
        XCTAssertEqual(word.text(cells: cells, cols: 10, rows: 1), "中文", "the continuation half belongs to the word")
    }

    func testOutOfRangePointsAreClampedNotTrapped() {
        let cells = grid(["abc"], cols: 4)
        var selection = TerminalSelection(at: point(-3, -1))
        selection.head = point(9, 99)
        XCTAssertEqual(selection.text(cells: cells, cols: 4, rows: 1), "abc")
        XCTAssertEqual(TerminalSelection(at: point(0, 0)).text(cells: [], cols: 4, rows: 1), "")
    }
}
