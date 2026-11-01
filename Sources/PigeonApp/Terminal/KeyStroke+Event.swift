import AppKit
import Carbon

extension KeyStroke {
    /// Maps an AppKit event onto the pure [`KeyStroke`] model.
    init?(_ event: NSEvent) {
        let modifiers = event.modifierFlags
        let control = modifiers.contains(.control)
        let option = modifiers.contains(.option)
        let shift = modifiers.contains(.shift)
        let command = modifiers.contains(.command)

        let key: KeyStroke.Key
        var base: Character?
        switch event.keyCode {
        case 36, 76: key = .enter
        case 48: key = .tab
        case 51: key = .backspace
        case 53: key = .escape
        case 117: key = .delete
        case 123: key = .left
        case 124: key = .right
        case 125: key = .down
        case 126: key = .up
        case 115: key = .home
        case 119: key = .end
        case 116: key = .pageUp
        case 121: key = .pageDown
        case 122: key = .function(1)
        case 120: key = .function(2)
        case 99: key = .function(3)
        case 118: key = .function(4)
        case 96: key = .function(5)
        case 97: key = .function(6)
        case 98: key = .function(7)
        case 100: key = .function(8)
        case 101: key = .function(9)
        case 109: key = .function(10)
        case 103: key = .function(11)
        case 111: key = .function(12)
        default:
            guard let characters = event.charactersIgnoringModifiers,
                  let first = characters.first
            else { return nil }
            key = .character(first)
            // Only a non-ASCII character needs the ASCII layout's view of the key; the
            // lookup is skipped for the common case.
            if first.asciiValue == nil {
                base = Self.asciiCharacter(keyCode: event.keyCode)
            }
        }

        self.init(
            key: key,
            control: control,
            option: option,
            shift: shift,
            command: command,
            base: base
        )
    }

    /// What `keyCode` types, unmodified, on the user's ASCII-capable keyboard layout
    /// (the layout macOS itself falls back to for shortcuts; Ghostty does the same).
    /// Nil when the key types nothing or something that is still not ASCII.
    static func asciiCharacter(keyCode: UInt16) -> Character? {
        guard let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?
                .takeRetainedValue(),
              let property = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData)
        else { return nil }
        let data = Unmanaged<CFData>.fromOpaque(property).takeUnretainedValue() as Data
        var deadKeys: UInt32 = 0
        var length = 0
        var chars = [UniChar](repeating: 0, count: 4)
        let status = data.withUnsafeBytes { raw -> OSStatus in
            guard let layout = raw.baseAddress?.assumingMemoryBound(to: UCKeyboardLayout.self)
            else { return -1 }
            return UCKeyTranslate(
                layout, keyCode, UInt16(kUCKeyActionDown), 0, UInt32(LMGetKbdType()),
                OptionBits(kUCKeyTranslateNoDeadKeysBit), &deadKeys, chars.count, &length, &chars)
        }
        guard status == noErr, length > 0 else { return nil }
        let text = String(utf16CodeUnits: chars, count: length)
        guard text.count == 1, let first = text.first, first.asciiValue != nil else { return nil }
        return first
    }
}
