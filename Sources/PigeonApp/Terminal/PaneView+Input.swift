import AppKit

/// Text input: printable keys go through the input method so pinyin, kana, dead keys and the
/// emoji panel work; everything a terminal encodes itself (arrows, Ctrl chords, ⌥ as Meta)
/// skips it and goes straight to `KeyEncoder`.
extension PaneView: NSTextInputClient {
    /// Whether `event` belongs to the input method rather than to `KeyEncoder`.
    func routesThroughInputMethod(_ event: NSEvent, stroke: KeyStroke) -> Bool {
        // While composing, every key (Return to commit, Esc to cancel, arrows to pick a
        // candidate) is the input method's.
        if hasMarkedText() { return true }
        guard case .character = stroke.key, !stroke.control, !stroke.command else { return false }
        return !(stroke.option && optionAsMeta)
    }

    func sendText(_ text: String) {
        guard !text.isEmpty else { return }
        sendBytes(Array(text.utf8))
    }

    private func setPreedit(_ value: Preedit?) {
        guard value != preedit else { return }
        preedit = value
        // The overlay may have moved off a row, which then has to be redrawn as the grid.
        dirty.markAll()
        screenChanged()
    }

    // MARK: NSTextInputClient

    func insertText(_ string: Any, replacementRange: NSRange) {
        setPreedit(nil)
        sendText(Self.plainString(string))
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let text = Self.plainString(string)
        setPreedit(text.isEmpty ? nil : Preedit(text: text, caret: selectedRange.location))
    }

    /// AppKit asks to finish composition (focus change, click): keep what was typed.
    func unmarkText() {
        guard let text = preedit?.text else { return }
        setPreedit(nil)
        sendText(text)
    }

    func selectedRange() -> NSRange {
        guard let preedit else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: preedit.caret, length: 0)
    }

    func markedRange() -> NSRange {
        guard let preedit else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: 0, length: preedit.text.utf16.count)
    }

    func hasMarkedText() -> Bool { preedit != nil }

    /// The grid is not a text store the input method can read back or edit.
    func attributedSubstring(
        forProposedRange range: NSRange, actualRange: NSRangePointer?
    ) -> NSAttributedString? { nil }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    /// The candidate window opens under the cursor cell (or the composing caret).
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        let cell = CoreTextRenderer.cellSize()
        var column = cursor.x
        var row = cursor.y
        if let place = preedit?.placement(cursor: cursor, cols: cols, rows: rows) {
            column = place.caretColumn
            row = place.row
        }
        let local = NSRect(
            x: CGFloat(column) * cell.width,
            y: bounds.height - CGFloat(row + 1) * cell.height,
            width: cell.width, height: cell.height)
        guard let window else { return local }
        return window.convertToScreen(convert(local, to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int { NSNotFound }

    /// Keys the input method did not consume come back as commands (`insertNewline:`,
    /// `deleteBackward:`); the terminal encodes the original key instead.
    override func doCommand(by selector: Selector) {
        guard let event = pendingKeyEvent, let stroke = KeyStroke(event),
              let bytes = KeyEncoder.encode(stroke, appCursor: modes & TerminalModes.appCursor != 0)
        else { return }
        sendBytes(bytes)
    }

    func sendBytes(_ bytes: [UInt8]) {
        clearSelection()
        // Typing while scrolled back snaps to the live screen, like every terminal.
        onScrollToBottom?()
        onInput?(bytes)
    }

    private static func plainString(_ value: Any) -> String {
        if let attributed = value as? NSAttributedString { return attributed.string }
        return value as? String ?? ""
    }
}
