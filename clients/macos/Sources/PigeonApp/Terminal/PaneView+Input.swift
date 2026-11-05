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
        // DECKPAM: keypad digits are keys the program binds, not text.
        if stroke.keypad, modes & TerminalModes.appKeypad != 0 { return false }
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
        // `postFrame` marks the row the overlay left and the row it now covers.
        screenChanged()
    }

    // MARK: NSTextInputClient

    func insertText(_ string: Any, replacementRange: NSRange) {
        let composed = preedit != nil
        setPreedit(nil)
        let text = Self.plainString(string)
        // A key that typed its own character (no composition) is still a key event for a
        // program that asked for every key as an escape code; composed text stays text.
        if !composed, let event = pendingKeyEvent, var stroke = KeyStroke(event),
           KittyKeyEncoder.Flags(modes: modes).contains(.allKeys) {
            stroke.text = text
            if let bytes = KeyEncoder.encode(stroke, modes: modes) {
                sendBytes(bytes)
                return
            }
        }
        sendText(text)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        // The replacement range is within the composition; the grid is not a text store the
        // input method can edit, so outside a composition it means "replace all of it".
        let (text, offset) = Preedit.replacement(
            replacementRange, in: preedit?.text, with: Self.plainString(string))
        let caret = selectedRange.location == NSNotFound ? text.utf16.count : offset + selectedRange.location
        setPreedit(text.isEmpty ? nil : Preedit(text: text, caret: min(caret, text.utf16.count)))
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
        let cell = cellSize
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
              let bytes = KeyEncoder.encode(stroke, modes: modes)
        else { return }
        sendBytes(bytes)
    }

    /// Releases only matter to a program that asked for kitty event types (flag 2).
    override func keyUp(with event: NSEvent) {
        guard KittyKeyEncoder.Flags(modes: modes).contains(.eventTypes), !hasMarkedText(),
              let stroke = KeyStroke(event),
              let bytes = KeyEncoder.encode(stroke, modes: modes)
        else { return super.keyUp(with: event) }
        onInput?(bytes)
    }

    /// A modifier key alone: reported only under kitty's "all keys as escape codes" (flag 8).
    override func flagsChanged(with event: NSEvent) {
        refreshLinkHover()
        guard KittyKeyEncoder.Flags(modes: modes).contains(.allKeys), !hasMarkedText(),
              let stroke = KeyStroke(modifierEvent: event),
              let bytes = KeyEncoder.encode(stroke, modes: modes)
        else { return super.flagsChanged(with: event) }
        onInput?(bytes)
    }

    func sendBytes(_ bytes: [UInt8]) {
        blink.typed(at: Date())
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
