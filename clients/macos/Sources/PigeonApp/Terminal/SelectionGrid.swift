import PigeonCore

/// Scoped snapshot adapter. All borrowed pointers expire when pg_select_grid returns;
/// the returned Swift result owns both text and highlight spans.
enum SelectionGrid {
    struct Result: Equatable {
        var text: String
        var spans: [SelectionSpan]
        static let empty = Result(text: "", spans: [])
    }

    static func select(_ selection: TerminalSelection, cells: [TerminalCell], cols: Int, rows: Int) -> Result {
        guard (1...Int(UInt16.max)).contains(cols), (1...Int(UInt16.max)).contains(rows),
              rows <= 1_000_000 / cols, cells.count >= cols * rows else { return .empty }
        let count = cols * rows
        var rawCells: [PgCell] = []
        rawCells.reserveCapacity(count)
        var textBytes: [UInt8] = []
        var textOffsets: [(index: Int, start: Int, length: Int)] = []
        for (index, cell) in cells.prefix(count).enumerated() {
            rawCells.append(PgCell(size: UInt32(MemoryLayout<PgCell>.size),
                codepoint: cell.codepoint == 0 ? 32 : cell.codepoint,
                fg: cell.fg, bg: cell.bg, attrs: UInt32(cell.attrs), hyperlink_id: 0, width: cell.width))
            if let text = cell.extraText {
                let size = text.utf8.count
                guard size <= 4096, size <= 8 * 1024 * 1024 - textBytes.count else { return .empty }
                textOffsets.append((index, textBytes.count, size))
                textBytes.append(contentsOf: text.utf8)
            }
        }
        var request = PgGridSelection()
        request.size = UInt32(MemoryLayout<PgGridSelection>.size)
        request.version = UInt32(PG_CORE_ABI_VERSION)
        request.anchor_row = UInt16(min(max(selection.anchor.row, 0), rows - 1))
        request.anchor_col = UInt16(min(max(selection.anchor.column, 0), cols - 1))
        request.head_row = UInt16(min(max(selection.head.row, 0), rows - 1))
        request.head_col = UInt16(min(max(selection.head.column, 0), cols - 1))
        switch selection.unit {
        case .cell: request.unit = 0
        case .word: request.unit = 1
        case .line: request.unit = 2
        }
        request.rectangle = selection.rectangle
        return rawCells.withUnsafeBufferPointer { raw in
            guard let base = raw.baseAddress else { return .empty }
            let rawRows = (0..<rows).map { row in
                PgRow(size: UInt32(MemoryLayout<PgRow>.size), cells: base.advanced(by: row * cols))
            }
            return textBytes.withUnsafeBufferPointer { bytes in
                let entries = textOffsets.map { offset in
                    PgCellText(size: UInt32(MemoryLayout<PgCellText>.size),
                        row: Int32(offset.index / cols), col: UInt16(offset.index % cols),
                        text: PgStr(size: UInt32(MemoryLayout<PgStr>.size),
                            ptr: bytes.baseAddress?.advanced(by: offset.start), len: offset.length))
                }
                return entries.withUnsafeBufferPointer { text in
                    rawRows.withUnsafeBufferPointer { screen in
                        var snapshot = PgSnapshot()
                        snapshot.size = UInt32(MemoryLayout<PgSnapshot>.size)
                        snapshot.version = UInt32(PG_CORE_ABI_VERSION)
                        snapshot.cols = UInt16(cols); snapshot.rows = UInt16(rows)
                        snapshot.screen = screen.baseAddress
                        snapshot.text = text.baseAddress; snapshot.text_count = UInt32(text.count)
                        guard let handle = pg_select_grid(&snapshot, &request) else { return .empty }
                        defer { pg_selection_free(handle) }
                        let value = pg_selection_text(handle)
                        let copied = value.ptr.map {
                            String(decoding: UnsafeBufferPointer(start: $0, count: value.len), as: UTF8.self)
                        } ?? ""
                        let count = pg_selection_spans(handle, nil, 0)
                        var spans = [PgSelectionSpan](repeating: PgSelectionSpan(), count: count)
                        let written = spans.withUnsafeMutableBufferPointer {
                            pg_selection_spans(handle, $0.baseAddress, $0.count)
                        }
                        guard written == count else { return .empty }
                        return Result(text: copied, spans: spans.map {
                            SelectionSpan(row: Int($0.row), first: Int($0.first), last: Int($0.end) - 1)
                        })
                    }
                }
            }
        }
    }
}

extension TerminalSelection {
    func resolve(cells: [TerminalCell], cols: Int, rows: Int) -> SelectionGrid.Result {
        SelectionGrid.select(self, cells: cells, cols: cols, rows: rows)
    }

    func text(cells: [TerminalCell], cols: Int, rows: Int) -> String {
        resolve(cells: cells, cols: cols, rows: rows).text
    }
}
