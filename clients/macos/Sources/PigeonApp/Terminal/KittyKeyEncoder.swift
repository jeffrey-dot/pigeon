import Foundation

/// The kitty keyboard protocol (https://sw.kovidgoyal.net/kitty/keyboard-protocol/), used once a
/// program pushed `CSI > flags u`. The core tracks the flag stack and forwards the active flags
/// in the modes word; this only maps one key event onto `CSI code:alternates ; mods:type ; text u`
/// (or the `CSI n ~` / `CSI 1 X` forms of functional keys).
enum KittyKeyEncoder {
    struct Flags: OptionSet, Equatable {
        let rawValue: UInt32
        init(rawValue: UInt32) { self.rawValue = rawValue }

        static let disambiguate = Flags(rawValue: 1)
        static let eventTypes = Flags(rawValue: 2)
        static let alternateKeys = Flags(rawValue: 4)
        static let allKeys = Flags(rawValue: 8)
        static let associatedText = Flags(rawValue: 16)

        /// The flags carried in a `TerminalModes` word (bits 10–14).
        init(modes: UInt32) {
            rawValue = (modes & TerminalModes.kittyFlags) >> TerminalModes.kittyShift
        }
    }

    enum Output: Equatable {
        /// The protocol keeps the legacy encoding for this event (plain text, unmodified Enter).
        case legacy
        case bytes([UInt8])
        /// Nothing is sent (a release the program did not ask for).
        case none
    }

    static func encode(_ stroke: KeyStroke, flags: Flags) -> Output {
        let release = stroke.action == .release
        if release, !flags.contains(.eventTypes) { return .none }
        let allKeys = flags.contains(.allKeys)
        let disambiguate = allKeys || flags.contains(.disambiguate)

        switch stroke.key {
        case let .modifier(modifier):
            guard allKeys else { return .none }
            return .bytes(sequence(key: "\(modifier.kittyCode)", final: "u", stroke, flags))
        case let .character(character):
            return textKey(character, stroke, flags)
        case .enter, .tab, .backspace:
            // Unmodified Enter/Tab/Backspace stay legacy so `reset⏎` still works after a crash;
            // they never report release unless every key is an escape code.
            if release, !allKeys { return .none }
            let unmodified = modifierBits(stroke) & ~lockBits == 0
            guard allKeys || (disambiguate && !unmodified) else { return .legacy }
            let enter = stroke.keypad ? 57414 : 13
            let code = stroke.key == .enter ? enter : stroke.key == .tab ? 9 : 127
            return .bytes(sequence(key: "\(code)", final: "u", stroke, flags))
        case .escape:
            guard disambiguate else { return release ? .none : .legacy }
            return .bytes(sequence(key: "27", final: "u", stroke, flags))
        default:
            guard let (number, final) = functional(stroke.key) else { return .none }
            // An unmodified press reads the same either way; legacy keeps DECCKM's `SS3`.
            // F13+ (`u` forms) have no legacy equivalent in kitty's table.
            if !allKeys, final != "u", stroke.action == .press, modifierBits(stroke) & ~lockBits == 0 {
                return .legacy
            }
            return .bytes(sequence(key: number, final: final, stroke, flags))
        }
    }

    // MARK: Text-producing keys

    private static func textKey(_ character: Character, _ input: KeyStroke, _ flags: Flags) -> Output {
        var stroke = input
        // Lock modifiers are only reported on text keys once every key is an escape code.
        if !flags.contains(.allKeys) { stroke.capsLock = false }
        let release = stroke.action == .release
        let allKeys = flags.contains(.allKeys)
        let disambiguate = allKeys || flags.contains(.disambiguate)
        // Plain and shifted keys are text; alt/ctrl chords become escape codes so Ctrl+I and
        // Tab, or Alt+[ and a CSI, can be told apart.
        let chord = stroke.control || stroke.option
        guard allKeys || (disambiguate && chord) else { return release ? .none : .legacy }

        let typed = character.unicodeScalars.first?.value ?? 0
        let code = stroke.keypad ? (keypadCodes[character] ?? unshifted(character, stroke))
            : unshifted(character, stroke)
        var key = "\(code)"
        if flags.contains(.alternateKeys) {
            let shifted = stroke.shift && typed != code ? "\(typed)" : ""
            var base = ""
            if let layout = stroke.base?.unicodeScalars.first?.value, layout != code {
                base = "\(layout)"
            }
            if !base.isEmpty { key += ":\(shifted):\(base)" } else if !shifted.isEmpty { key += ":\(shifted)" }
        }

        var text: String?
        if allKeys, flags.contains(.associatedText), !release {
            // `text` comes from the input method (⌥ composing `å`); a Ctrl chord types nothing.
            let source = stroke.text ?? (chord ? "" : String(character))
            let printable = source.unicodeScalars.filter { !isControl($0) }.map { "\($0.value)" }
            if !printable.isEmpty { text = printable.joined(separator: ":") }
        }
        return .bytes(sequence(key: key, final: "u", stroke, flags, text: text))
    }

    /// The key's code is always its unshifted form: Shift+A reports `a`, Shift+1 reports `1`.
    private static func unshifted(_ character: Character, _ stroke: KeyStroke) -> UInt32 {
        if let unshifted = stroke.unshifted?.unicodeScalars.first?.value { return unshifted }
        let lower = character.lowercased()
        let scalars = lower.count == 1 ? lower.unicodeScalars : character.unicodeScalars
        return scalars.first?.value ?? 0
    }

    /// C0, DEL and C1 never travel as associated text.
    private static func isControl(_ scalar: Unicode.Scalar) -> Bool {
        scalar.value < 0x20 || (0x7f...0x9f).contains(scalar.value)
    }

    // MARK: Modifiers and framing

    private static let lockBits = 64

    /// shift 1, alt 2, ctrl 4, super 8, caps lock 64; the wire value is 1 + these bits.
    private static func modifierBits(_ stroke: KeyStroke) -> Int {
        (stroke.shift ? 1 : 0) + (stroke.option ? 2 : 0) + (stroke.control ? 4 : 0)
            + (stroke.command ? 8 : 0) + (stroke.capsLock ? lockBits : 0)
    }

    /// `CSI key ; mods:type ; text final`, dropping trailing fields that carry nothing. The
    /// `CSI 1 X` forms drop the `1` too when no modifier or event type follows.
    private static func sequence(
        key: String, final: Character, _ stroke: KeyStroke, _ flags: Flags, text: String? = nil
    ) -> [UInt8] {
        let mods = 1 + modifierBits(stroke)
        var type: Int?
        if flags.contains(.eventTypes) {
            switch stroke.action {
            case .press: type = nil
            case .repeated: type = 2
            case .release: type = 3
            }
        }
        var field = ""
        if mods > 1 || type != nil { field = "\(mods)" }
        if let type { field += ":\(type)" }

        var out = "\u{1b}["
        if field.isEmpty, text == nil, key == "1", final != "u", final != "~" {
            out += "\(final)"
            return Array(out.utf8)
        }
        out += key
        if !field.isEmpty || text != nil { out += ";\(field)" }
        if let text { out += ";\(text)" }
        out += "\(final)"
        return Array(out.utf8)
    }

    // MARK: Tables

    /// Functional keys as (number, final): `CSI number ; mods final`.
    private static func functional(_ key: KeyStroke.Key) -> (String, Character)? {
        switch key {
        case .insert: return ("2", "~")
        case .delete: return ("3", "~")
        case .pageUp: return ("5", "~")
        case .pageDown: return ("6", "~")
        case .up: return ("1", "A")
        case .down: return ("1", "B")
        case .right: return ("1", "C")
        case .left: return ("1", "D")
        case .home: return ("1", "H")
        case .end: return ("1", "F")
        case let .function(number):
            switch number {
            case 1: return ("1", "P")
            case 2: return ("1", "Q")
            // F3 has no `CSI R` form: that is a cursor position report.
            case 3: return ("13", "~")
            case 4: return ("1", "S")
            case 5...12:
                let codes = [15, 17, 18, 19, 20, 21, 23, 24]
                return ("\(codes[number - 5])", "~")
            case 13...35: return ("\(57376 + number - 13)", "u")
            default: return nil
            }
        default:
            return nil
        }
    }

    /// `KP_*` codes for keypad keys that also type text.
    private static let keypadCodes: [Character: UInt32] = [
        "0": 57399, "1": 57400, "2": 57401, "3": 57402, "4": 57403, "5": 57404, "6": 57405,
        "7": 57406, "8": 57407, "9": 57408, ".": 57409, "/": 57410, "*": 57411, "-": 57412,
        "+": 57413, "=": 57415, ",": 57416,
    ]
}

extension KeyStroke.Modifier {
    var kittyCode: Int {
        switch self {
        case .leftShift: return 57441
        case .leftControl: return 57442
        case .leftOption: return 57443
        case .leftCommand: return 57444
        case .rightShift: return 57447
        case .rightControl: return 57448
        case .rightOption: return 57449
        case .rightCommand: return 57450
        case .capsLock: return 57358
        }
    }
}
