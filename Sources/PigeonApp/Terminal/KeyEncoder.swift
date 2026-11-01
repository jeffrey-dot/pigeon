import Foundation

/// Turns a key stroke into the bytes a terminal expects: xterm's encoding, including its
/// modifier parameter (`CSI 1;5C` for Ctrl+→) and application cursor mode (DECCKM), which
/// is what readline, vim and fish actually bind. The kitty keyboard protocol is later.
public enum KeyEncoder {
    /// `appCursor`: the program turned on DECCKM (`?1h`), so unmodified arrows, Home and
    /// End go out as `SS3` (`ESC O A`) instead of `CSI`.
    public static func encode(_ stroke: KeyStroke, appCursor: Bool = false) -> [UInt8]? {
        // Command chords belong to the app, not the shell.
        if stroke.command { return nil }

        if let sequence = special(stroke, appCursor: appCursor) {
            return sequence
        }

        guard case var .character(character) = stroke.key else { return nil }

        // Ctrl+С on a Russian layout means Ctrl+C: control bytes only exist for ASCII.
        if stroke.control, character.asciiValue == nil, let base = stroke.base {
            character = base
        }

        if stroke.control, let byte = controlByte(character) {
            return stroke.option ? [0x1b, byte] : [byte]
        }

        var bytes = Array(String(character).utf8)
        if stroke.option {
            bytes.insert(0x1b, at: 0)
        }
        return bytes
    }

    private static func controlByte(_ character: Character) -> UInt8? {
        guard let ascii = character.asciiValue else { return nil }
        switch ascii {
        case UInt8(ascii: "a")...UInt8(ascii: "z"):
            return ascii - UInt8(ascii: "a") + 1
        case UInt8(ascii: "A")...UInt8(ascii: "Z"):
            return ascii - UInt8(ascii: "A") + 1
        case UInt8(ascii: "["), UInt8(ascii: "\\"), UInt8(ascii: "]"),
             UInt8(ascii: "^"), UInt8(ascii: "_"), UInt8(ascii: "@"):
            return ascii - 0x40
        case UInt8(ascii: "?"), UInt8(ascii: "8"):
            return 0x7f
        case UInt8(ascii: " "), UInt8(ascii: "2"):
            return 0x00
        // xterm's VT220-style digits: Ctrl+3…7 are ESC, FS, GS, RS, US.
        case UInt8(ascii: "3")...UInt8(ascii: "7"):
            return ascii - UInt8(ascii: "3") + 0x1b
        case UInt8(ascii: "/"):
            return 0x1f
        case UInt8(ascii: "~"):
            return 0x1e
        default:
            return nil
        }
    }

    /// xterm's modifier parameter: 1 + shift + 2·alt + 4·ctrl; 1 means "no modifier".
    private static func modifier(_ stroke: KeyStroke) -> Int {
        1 + (stroke.shift ? 1 : 0) + (stroke.option ? 2 : 0) + (stroke.control ? 4 : 0)
    }

    /// Cursor-style keys: `CSI 1;m X` with modifiers, else `SS3 X` / `CSI X`.
    private static func cursorKey(_ final: Character, _ stroke: KeyStroke, appCursor: Bool) -> [UInt8] {
        let mod = modifier(stroke)
        if mod > 1 { return Array("\u{1b}[1;\(mod)\(final)".utf8) }
        return Array((appCursor ? "\u{1b}O\(final)" : "\u{1b}[\(final)").utf8)
    }

    /// Tilde keys: `CSI n ~`, or `CSI n;m ~` with modifiers.
    private static func tildeKey(_ number: Int, _ stroke: KeyStroke) -> [UInt8] {
        let mod = modifier(stroke)
        return Array((mod > 1 ? "\u{1b}[\(number);\(mod)~" : "\u{1b}[\(number)~").utf8)
    }

    private static func special(_ stroke: KeyStroke, appCursor: Bool) -> [UInt8]? {
        let alt: [UInt8] = stroke.option ? [0x1b] : []
        switch stroke.key {
        case .enter:
            return alt + [0x0d]
        case .tab:
            return stroke.shift ? Array("\u{1b}[Z".utf8) : alt + [0x09]
        case .backspace:
            // ⌥⌫ is "delete word" in readline and zsh: ESC DEL.
            return alt + [stroke.control ? 0x08 : 0x7f]
        case .escape:
            return alt + [0x1b]
        case .delete:
            return tildeKey(3, stroke)
        case .up:
            return cursorKey("A", stroke, appCursor: appCursor)
        case .down:
            return cursorKey("B", stroke, appCursor: appCursor)
        case .right:
            return cursorKey("C", stroke, appCursor: appCursor)
        case .left:
            return cursorKey("D", stroke, appCursor: appCursor)
        case .home:
            return cursorKey("H", stroke, appCursor: appCursor)
        case .end:
            return cursorKey("F", stroke, appCursor: appCursor)
        case .pageUp:
            return tildeKey(5, stroke)
        case .pageDown:
            return tildeKey(6, stroke)
        case .insert:
            return tildeKey(2, stroke)
        case let .function(number) where (13...20).contains(number):
            // xterm-256color's kf13–kf20 are Shift+F1–F8.
            var shifted = stroke
            shifted.key = .function(number - 12)
            shifted.shift = true
            return special(shifted, appCursor: appCursor)
        case let .function(number):
            if (1...4).contains(number) {
                // F1–F4 are SS3 P/Q/R/S, or `CSI 1;m P` once modified.
                let final = Character(Unicode.Scalar(UInt8(ascii: "P") + UInt8(number - 1)))
                let mod = modifier(stroke)
                return Array((mod > 1 ? "\u{1b}[1;\(mod)\(final)" : "\u{1b}O\(final)").utf8)
            }
            let codes = [15, 17, 18, 19, 20, 21, 23, 24]
            guard number >= 5, number <= 12 else { return nil }
            return tildeKey(codes[number - 5], stroke)
        case .character:
            return nil
        }
    }
}
