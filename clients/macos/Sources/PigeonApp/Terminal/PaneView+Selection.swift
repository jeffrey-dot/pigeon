import AppKit

/// Mouse selection and the clipboard: drag selects, double-click a word, triple-click a
/// line; ⌘C copies, ⌘V pastes through the host (which brackets it when the program asks).
extension PaneView {
    /// The cell under a window-space event, clamped to the grid.
    func gridPoint(for event: NSEvent) -> TerminalSelection.Point {
        let local = convert(event.locationInWindow, from: nil)
        let size = CoreTextRenderer.cellSize()
        let column = Int((local.x / max(size.width, 1)).rounded(.down))
        // AppKit's origin is bottom-left; row 0 is the top line.
        let row = Int(((bounds.height - local.y) / max(size.height, 1)).rounded(.down))
        return TerminalSelection.Point(
            row: min(max(row, 0), max(rows - 1, 0)),
            column: min(max(column, 0), max(cols - 1, 0)))
    }

    func beginSelection(with event: NSEvent) {
        let unit: TerminalSelection.Unit
        switch event.clickCount {
        case 2: unit = .word
        case 3...: unit = .line
        default: unit = .cell
        }
        selection = TerminalSelection(at: gridPoint(for: event), unit: unit)
        // A plain click only places the anchor; nothing is selected until the pointer moves.
        selectionActive = unit != .cell
        screenChanged()
    }

    func extendSelection(with event: NSEvent) {
        guard var current = selection else { return }
        let point = gridPoint(for: event)
        guard point != current.head || selectionActive else { return }
        current.head = point
        selection = current
        selectionActive = true
        screenChanged()
    }

    func clearSelection() {
        guard selection != nil else { return }
        selection = nil
        selectionActive = false
        screenChanged()
    }

    /// What the renderer highlights, one span per selected row.
    func selectionSpans() -> [SelectionSpan] {
        guard selectionActive, let selection, cols > 0 else { return [] }
        let (start, end) = selection.range(cells: cells, cols: cols)
        guard start.row <= end.row else { return [] }
        return (max(start.row, 0)...min(end.row, rows - 1)).map { row in
            SelectionSpan(
                row: row,
                first: row == start.row ? start.column : 0,
                last: row == end.row ? end.column : cols - 1)
        }
    }

    var selectedText: String {
        guard selectionActive, let selection else { return "" }
        return selection.text(cells: cells, cols: cols, rows: rows)
    }

    @objc func copy(_ sender: Any?) {
        let text = selectedText
        guard !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    @objc func paste(_ sender: Any?) {
        guard let text = NSPasteboard.general.string(forType: .string), !text.isEmpty else { return }
        onScrollToBottom?()
        onPaste?(text)
    }

    @objc override func selectAll(_ sender: Any?) {
        guard rows > 0, cols > 0 else { return }
        var all = TerminalSelection(at: .init(row: 0, column: 0))
        all.head = .init(row: rows - 1, column: cols - 1)
        selection = all
        selectionActive = true
        screenChanged()
    }

    @objc func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(copy(_:)): return !selectedText.isEmpty
        case #selector(paste(_:)): return NSPasteboard.general.string(forType: .string) != nil
        default: return true
        }
    }
}
