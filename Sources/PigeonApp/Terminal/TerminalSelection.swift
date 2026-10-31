/// A text selection on a pane's grid, and the text it covers. Pure: no AppKit, so the rules
/// (word boundaries, wide characters, trailing blanks, line joins) are unit-tested directly.
///
/// Points are grid cells, `row` 0 at the top. The selection is linear, like every terminal's
/// default: from `anchor` to `head` in reading order, whole rows in between.
struct TerminalSelection: Equatable {
    struct Point: Equatable, Comparable {
        var row: Int
        var column: Int

        static func < (lhs: Point, rhs: Point) -> Bool {
            lhs.row != rhs.row ? lhs.row < rhs.row : lhs.column < rhs.column
        }
    }

    /// How a drag extends: by cell, by word (double-click) or by line (triple-click).
    enum Unit: Equatable {
        case cell
        case word
        case line
    }

    var anchor: Point
    var head: Point
    var unit: Unit

    init(at point: Point, unit: Unit = .cell) {
        anchor = point
        head = point
        self.unit = unit
    }

    /// Start and end in reading order, both inclusive, after snapping to `unit`.
    func range(cells: [PaneView.Cell], cols: Int) -> (start: Point, end: Point) {
        let (low, high) = anchor < head ? (anchor, head) : (head, anchor)
        switch unit {
        case .cell:
            return (low, high)
        case .line:
            return (Point(row: low.row, column: 0), Point(row: high.row, column: max(cols - 1, 0)))
        case .word:
            let start = Self.wordBounds(at: low, cells: cells, cols: cols).start
            let end = Self.wordBounds(at: high, cells: cells, cols: cols).end
            return (start, end)
        }
    }

    func contains(_ point: Point, cells: [PaneView.Cell], cols: Int) -> Bool {
        let (start, end) = range(cells: cells, cols: cols)
        return start <= point && point <= end
    }

    /// The selected text. Trailing blanks on each row are dropped (a terminal row is padded
    /// to its width), rows join with `\n`, and a wide character's continuation cell adds
    /// nothing, so `中文` copies as two characters, not four.
    func text(cells: [PaneView.Cell], cols: Int, rows: Int) -> String {
        guard cols > 0, rows > 0, cells.count >= cols * rows else { return "" }
        let (start, end) = range(cells: cells, cols: cols)
        var lines: [String] = []
        for row in max(start.row, 0)...min(end.row, rows - 1) {
            let first = row == start.row ? max(start.column, 0) : 0
            let last = row == end.row ? min(end.column, cols - 1) : cols - 1
            guard first <= last else {
                lines.append("")
                continue
            }
            var line = ""
            for column in first...last {
                let cell = cells[row * cols + column]
                guard cell.width != 0 else { continue }
                if cell.codepoint == 0 {
                    line.append(" ")
                } else if let scalar = Unicode.Scalar(cell.codepoint) {
                    line.unicodeScalars.append(scalar)
                }
            }
            while line.last == " " { line.removeLast() }
            lines.append(line)
        }
        return lines.joined(separator: "\n")
    }

    /// The run of word characters around `point`; a non-word cell selects just itself.
    /// Path and URL punctuation counts as part of a word, so a double-click takes
    /// `src/main.rs:12` or `https://example.com/a?b=1` whole.
    static func wordBounds(at point: Point, cells: [PaneView.Cell], cols: Int) -> (start: Point, end: Point) {
        guard cols > 0, point.row >= 0, (point.row + 1) * cols <= cells.count else {
            return (point, point)
        }
        let column = min(max(point.column, 0), cols - 1)
        let base = point.row * cols
        func isWord(_ index: Int) -> Bool {
            let cell = cells[base + index]
            if cell.width == 0 { return true } // the right half of a wide character
            guard let scalar = Unicode.Scalar(cell.codepoint) else { return false }
            if scalar.properties.isAlphabetic || ("0"..."9").contains(scalar) { return true }
            return "_-./~:@%+=?&#".unicodeScalars.contains(scalar)
        }
        guard isWord(column) else {
            return (Point(row: point.row, column: column), Point(row: point.row, column: column))
        }
        var start = column
        while start > 0, isWord(start - 1) { start -= 1 }
        var end = column
        while end < cols - 1, isWord(end + 1) { end += 1 }
        return (Point(row: point.row, column: start), Point(row: point.row, column: end))
    }
}
