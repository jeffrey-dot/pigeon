/// Mouse events as the bytes a program asked for (`?1000`/`?1002`/`?1003` + `?1006`): pure,
/// so the encodings are tested without a window. Coordinates are 0-based cells here and
/// 1-based on the wire, as xterm sends them.
enum MouseEncoder {
    enum Button: Equatable {
        case left, middle, right
        case wheelUp, wheelDown
        /// Motion with no button held (only reported in `?1003`).
        case none
    }

    enum Action: Equatable {
        case press, release, drag, move
    }

    struct Modifiers: Equatable {
        var shift = false
        var option = false
        var control = false
    }

    /// The report for one event under `modes`, or nil when the program did not ask for it
    /// (a drag without `?1002`, motion without `?1003`, nothing at all without mouse mode).
    static func encode(
        _ button: Button, _ action: Action, column: Int, row: Int,
        modifiers: Modifiers = Modifiers(), modes: UInt32
    ) -> [UInt8]? {
        guard modes & TerminalModes.anyMouse != 0 else { return nil }
        switch action {
        case .drag where modes & (TerminalModes.mouseDrag | TerminalModes.mouseMotion) == 0:
            return nil
        case .move where modes & TerminalModes.mouseMotion == 0:
            return nil
        default:
            break
        }
        var code: Int
        switch button {
        case .left: code = 0
        case .middle: code = 1
        case .right: code = 2
        case .none: code = 3
        case .wheelUp: code = 64
        case .wheelDown: code = 65
        }
        if action == .drag || action == .move { code += 32 }
        if modifiers.shift { code += 4 }
        if modifiers.option { code += 8 }
        if modifiers.control { code += 16 }
        let x = max(column, 0) + 1
        let y = max(row, 0) + 1

        if modes & TerminalModes.mouseSGR != 0 {
            // SGR keeps the button on release and says so with `m`.
            let final = action == .release ? "m" : "M"
            return Array("\u{1b}[<\(code);\(x);\(y)\(final)".utf8)
        }
        // X10/normal: release is button 3, and coordinates ride as bytes offset by 32,
        // which caps them at 223; beyond that the report cannot be expressed.
        if action == .release { code = 3 + (code & ~3 & ~64) }
        guard x <= 223, y <= 223 else { return nil }
        return [0x1b, UInt8(ascii: "["), UInt8(ascii: "M"),
                UInt8(32 + code), UInt8(32 + x), UInt8(32 + y)]
    }
}
