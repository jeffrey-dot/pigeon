import AppKit
import Carbon

extension KeyStroke {
    /// Maps an AppKit key event (`keyDown`, `keyUp`) onto the pure [`KeyStroke`] model.
    init?(_ event: NSEvent) {
        let modifiers = event.modifierFlags
        let control = modifiers.contains(.control)
        let option = modifiers.contains(.option)
        let shift = modifiers.contains(.shift)
        let command = modifiers.contains(.command)

        let key: KeyStroke.Key
        var base: Character?
        var unshifted: Character?
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
        case 114: key = .insert  // the Help key on Apple keyboards sits where Insert is
        case 105: key = .function(13)
        case 107: key = .function(14)
        case 113: key = .function(15)
        case 106: key = .function(16)
        case 64: key = .function(17)
        case 79: key = .function(18)
        case 80: key = .function(19)
        case 90: key = .function(20)
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
            // Shift+1 types `!`; the kitty protocol reports the key as `1`.
            if shift, let plain = event.characters(byApplyingModifiers: [])?.first, plain != first {
                unshifted = plain
            }
        }

        self.init(
            key: key,
            control: control,
            option: option,
            shift: shift,
            command: command,
            capsLock: modifiers.contains(.capsLock),
            keypad: Self.keypadCodes.contains(event.keyCode),
            action: event.type == .keyUp ? .release : event.isARepeat ? .repeated : .press,
            base: base,
            unshifted: unshifted
        )
    }

    /// A `flagsChanged` event: one modifier key went down or up. Pressed is read from the
    /// device-dependent bits, so releasing one Shift while the other is held is a release.
    init?(modifierEvent event: NSEvent) {
        let modifiers = event.modifierFlags
        guard let (modifier, deviceBit) = Self.modifierKeys[event.keyCode] else { return nil }
        let pressed = modifier == .capsLock
            ? modifiers.contains(.capsLock)
            : modifiers.rawValue & deviceBit != 0
        self.init(
            key: .modifier(modifier),
            control: modifiers.contains(.control),
            option: modifiers.contains(.option),
            shift: modifiers.contains(.shift),
            command: modifiers.contains(.command),
            capsLock: modifiers.contains(.capsLock),
            action: pressed ? .press : .release)
    }

    /// Modifier key codes and their `NX_DEVICE*KEYMASK` bit in `modifierFlags`.
    private static let modifierKeys: [UInt16: (KeyStroke.Modifier, UInt)] = [
        56: (.leftShift, 0x02), 60: (.rightShift, 0x04),
        59: (.leftControl, 0x01), 62: (.rightControl, 0x2000),
        58: (.leftOption, 0x20), 61: (.rightOption, 0x40),
        55: (.leftCommand, 0x08), 54: (.rightCommand, 0x10),
        57: (.capsLock, 0),
    ]

    /// The numeric keypad's key codes (`kVK_ANSI_Keypad*`). `NSEvent.ModifierFlags.numericPad`
    /// cannot be used: AppKit sets it on the arrow keys too.
    static let keypadCodes: Set<UInt16> = [
        65, 67, 69, 75, 76, 78, 81, 82, 83, 84, 85, 86, 87, 88, 89, 91, 92,
    ]

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
