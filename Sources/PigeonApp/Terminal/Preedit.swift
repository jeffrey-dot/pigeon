/// Text an input method is still composing (pinyin before a candidate is picked, a dead key
/// before its vowel). It is drawn over the grid at the cursor, underlined, and never reaches
/// the host until the input method commits it. Pure, so placement is unit-tested.
struct Preedit: Equatable {
    var text: String
    /// The input method's caret inside `text`, in UTF-16 units (`NSRange.location`).
    var caret: Int

    /// Where the overlay sits on a row: its first column and the columns it covers.
    struct Placement: Equatable {
        var row: Int
        var column: Int
        var cells: [PaneView.Cell]
        /// Grid column of the input method's caret, where the cursor is drawn.
        var caretColumn: Int
    }

    /// Starts at the cursor; when the text would run past the right edge it slides left so the
    /// end (where the user is typing) stays visible. Text wider than the row keeps its tail.
    func placement(cursor: PaneView.Cursor, cols: Int, rows: Int) -> Placement? {
        guard !text.isEmpty, cols > 0, rows > 0 else { return nil }
        var cells: [PaneView.Cell] = []
        var caretColumn: Int?
        var units = 0
        for scalar in text.unicodeScalars {
            if units >= caret, caretColumn == nil { caretColumn = cells.count }
            units += scalar.utf16.count
            let width = CharacterWidth.of(scalar)
            guard width > 0 else { continue }
            cells.append(PaneView.Cell(
                codepoint: scalar.value, attrs: CellAttrs.underline, fg: 0, bg: 0, width: UInt8(width)))
            if width == 2 {
                cells.append(PaneView.Cell(
                    codepoint: 0, attrs: CellAttrs.underline, fg: 0, bg: 0, width: 0))
            }
        }
        var caretAt = caretColumn ?? cells.count
        if cells.count > cols {
            let drop = cells.count - cols
            cells.removeFirst(drop)
            // A wide character cut in half leaves an orphaned continuation cell.
            if cells.first?.width == 0 { cells[0] = PaneView.Cell(
                codepoint: 32, attrs: CellAttrs.underline, fg: 0, bg: 0, width: 1) }
            caretAt = max(caretAt - drop, 0)
        }
        let row = min(max(cursor.y, 0), rows - 1)
        let column = min(max(cursor.x, 0), cols - cells.count)
        return Placement(
            row: row, column: column, cells: cells,
            caretColumn: min(column + caretAt, cols - 1))
    }

    /// `cells` with the overlay written in, and the cursor moved to the composing caret.
    func overlay(
        cells: [PaneView.Cell], cursor: PaneView.Cursor, cols: Int, rows: Int
    ) -> (cells: [PaneView.Cell], cursor: PaneView.Cursor, row: Int)? {
        guard cells.count == cols * rows,
              let place = placement(cursor: cursor, cols: cols, rows: rows)
        else { return nil }
        var out = cells
        let base = place.row * cols + place.column
        for (offset, cell) in place.cells.enumerated() {
            out[base + offset] = cell
        }
        // The grid cell just after the overlay may be the right half of a wide character
        // whose left half is now covered; blank it rather than draw half a glyph.
        let after = place.column + place.cells.count
        if after < cols, out[place.row * cols + after].width == 0 {
            out[place.row * cols + after] = .blank
        }
        var moved = cursor
        moved.x = place.caretColumn
        moved.y = place.row
        moved.visible = true
        moved.shape = .bar
        return (out, moved, place.row)
    }
}
