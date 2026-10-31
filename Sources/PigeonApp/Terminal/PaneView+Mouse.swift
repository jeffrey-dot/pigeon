import AppKit

/// Mouse events for programs that asked for them (vim, htop, tmux in the pane). Shift keeps
/// the local selection available on top, as iTerm2 and Ghostty do.
extension PaneView {
    /// Whether this event is the program's: it turned mouse reporting on, and shift (the
    /// "I want to select text" override) is not held.
    func reportsMouse(_ event: NSEvent) -> Bool {
        modes & TerminalModes.anyMouse != 0 && !event.modifierFlags.contains(.shift)
    }

    func modifiers(of event: NSEvent) -> MouseEncoder.Modifiers {
        let flags = event.modifierFlags
        return .init(
            shift: flags.contains(.shift), option: flags.contains(.option),
            control: flags.contains(.control))
    }

    /// Sends one report; `false` when the program does not want this kind of event.
    @discardableResult
    func report(_ button: MouseEncoder.Button, _ action: MouseEncoder.Action, _ event: NSEvent) -> Bool {
        let point = gridPoint(for: event)
        guard let bytes = MouseEncoder.encode(
            button, action, column: point.column, row: point.row,
            modifiers: modifiers(of: event), modes: modes)
        else { return false }
        onInput?(bytes)
        return true
    }

    /// The wheel on the alternate screen: reported to a mouse-aware program, otherwise turned
    /// into arrow keys (`?1007`) so `less` and `man` scroll. Returns whether it was handled.
    func wheelOnAlternateScreen(_ event: NSEvent, lines: Int) -> Bool {
        guard alt, lines != 0 else { return false }
        if reportsMouse(event) {
            let button: MouseEncoder.Button = lines > 0 ? .wheelUp : .wheelDown
            for _ in 0..<min(abs(lines), 10) { report(button, .press, event) }
            return true
        }
        guard modes & TerminalModes.alternateScroll != 0 else { return false }
        let key: KeyStroke.Key = lines > 0 ? .up : .down
        guard let bytes = KeyEncoder.encode(
            KeyStroke(key: key), appCursor: modes & TerminalModes.appCursor != 0)
        else { return false }
        for _ in 0..<min(abs(lines), 10) { onInput?(bytes) }
        return true
    }
}
