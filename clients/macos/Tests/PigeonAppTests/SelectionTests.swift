import XCTest
@testable import PigeonApp

/// What a drag, double-click or triple-click selects, and the text ⌘C copies.
final class SelectionTests: XCTestCase {
    /// Rows of the same width; `中` takes two cells (the second a continuation).
    private func grid(_ lines: [String], cols: Int) -> [TerminalCell] {
        var cells: [TerminalCell] = []
        for line in lines {
            var row: [TerminalCell] = []
            for scalar in line.unicodeScalars {
                let wide = scalar.value >= 0x4E00 && scalar.value <= 0x9FFF
                row.append(TerminalCell(codepoint: scalar.value, attrs: 0, fg: 0, bg: 0, width: wide ? 2 : 1))
                if wide { row.append(TerminalCell(codepoint: 32, attrs: 0, fg: 0, bg: 0, width: 0)) }
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

    func testAutomaticWrapJoinsWithoutInventingANewline() {
        var cells = grid(["abcd", "efgh", "xy"], cols: 4)
        cells[3].attrs |= CellAttrs.wrap
        var selection = TerminalSelection(at: point(0, 0))
        selection.head = point(2, 1)
        XCTAssertEqual(selection.text(cells: cells, cols: 4, rows: 3), "abcdefgh\nxy")
    }

    func testCombiningMarksAreCopiedWithoutNormalization() {
        var cells = grid(["e"], cols: 4)
        cells[0].extraText = "e\u{301}"
        let selection = TerminalSelection(at: point(0, 0))
        XCTAssertEqual(Array(selection.text(cells: cells, cols: 4, rows: 1).unicodeScalars).map(\.value), [0x65, 0x301])
    }

    func testSpacesAtTheAutomaticWrapArePartOfTheCommand() {
        var cells = grid(["ab  ", "cd"], cols: 4)
        cells[3].attrs |= CellAttrs.wrap
        var selection = TerminalSelection(at: point(0, 0))
        selection.head = point(1, 1)
        XCTAssertEqual(selection.text(cells: cells, cols: 4, rows: 2), "ab  cd")
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

    func testDragStartingOnWideContinuationIncludesTheWholeCharacter() {
        var cells = grid(["a中e"], cols: 8)
        cells[3].extraText = "e\u{301}"
        var selection = TerminalSelection(at: point(0, 2))
        selection.head = point(0, 3)
        let result = selection.resolve(cells: cells, cols: 8, rows: 1)
        XCTAssertEqual(result.text, "中e\u{301}")
        XCTAssertEqual(result.spans, [SelectionSpan(row: 0, first: 1, last: 3)])
    }

    func testRectangleKeepsColumnsAndSpacesWithoutLinearWrapJoining() {
        var cells = grid(["ab  ", "cd  "], cols: 4)
        cells[3].attrs |= CellAttrs.wrap
        var selection = TerminalSelection(at: point(0, 1), rectangle: true)
        selection.head = point(1, 3)
        let result = selection.resolve(cells: cells, cols: 4, rows: 2)
        XCTAssertEqual(result.text, "b  \nd  ")
        XCTAssertEqual(result.spans, [.init(row: 0, first: 1, last: 3), .init(row: 1, first: 1, last: 3)])
    }

    func testCopiedSelectionOutlivesTemporaryCellAndTextStorage() {
        func selected() -> SelectionGrid.Result {
            var cells = grid(["e"], cols: 4)
            cells[0].extraText = "e\u{301}"
            return TerminalSelection(at: point(0, 0)).resolve(cells: cells, cols: 4, rows: 1)
        }
        let result = selected()
        XCTAssertEqual(result.text, "e\u{301}")
        XCTAssertEqual(result.spans, [.init(row: 0, first: 0, last: 0)])
    }

    func testMalformedGeometryAndOversizedCellTextFailWithoutPartialCopy() {
        let selection = TerminalSelection(at: point(Int.min, Int.min))
        let cells = grid(["abc"], cols: 4)
        for (cols, rows) in [(0, 1), (-1, 1), (4, -1), (Int.max, Int.max), (4, 2)] {
            XCTAssertEqual(selection.resolve(cells: cells, cols: cols, rows: rows), .empty)
        }
        var oversized = cells
        oversized[0].extraText = String(repeating: "x", count: 4097)
        XCTAssertEqual(selection.resolve(cells: oversized, cols: 4, rows: 1), .empty)
    }
}
