import Foundation

/// One key press, already decoded from AppKit. Pure data so it can be tested
/// without a window server.
public struct KeyStroke: Equatable {
    public enum Key: Equatable {
        case character(Character)
        case enter
        case tab
        case backspace
        case escape
        case delete
        case up
        case down
        case left
        case right
        case home
        case end
        case pageUp
        case pageDown
        case function(Int)
    }

    public var key: Key
    public var control: Bool
    public var option: Bool
    public var shift: Bool
    public var command: Bool

    public init(
        key: Key,
        control: Bool = false,
        option: Bool = false,
        shift: Bool = false,
        command: Bool = false
    ) {
        self.key = key
        self.control = control
        self.option = option
        self.shift = shift
        self.command = command
    }
}

/// Turns a key stroke into the bytes a terminal expects (legacy encoding; the
/// kitty keyboard protocol is a later milestone).
public enum KeyEncoder {
    public static func encode(_ stroke: KeyStroke) -> [UInt8]? {
        // Command chords belong to the app, not the shell.
        if stroke.command { return nil }

        if let sequence = special(stroke) {
            return stroke.option ? [0x1b] + sequence : sequence
        }

        guard case let .character(character) = stroke.key else { return nil }

        if stroke.control, let byte = controlByte(character) {
            return [byte]
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
        case UInt8(ascii: "?"):
            return 0x7f
        case UInt8(ascii: " "):
            return 0x00
        default:
            return nil
        }
    }

    private static func special(_ stroke: KeyStroke) -> [UInt8]? {
        switch stroke.key {
        case .enter:
            return [0x0d]
        case .tab:
            return stroke.shift ? [0x1b, UInt8(ascii: "["), UInt8(ascii: "Z")] : [0x09]
        case .backspace:
            return [0x7f]
        case .escape:
            return [0x1b]
        case .delete:
            return Array("\u{1b}[3~".utf8)
        case .up:
            return Array("\u{1b}[A".utf8)
        case .down:
            return Array("\u{1b}[B".utf8)
        case .right:
            return Array("\u{1b}[C".utf8)
        case .left:
            return Array("\u{1b}[D".utf8)
        case .home:
            return Array("\u{1b}[H".utf8)
        case .end:
            return Array("\u{1b}[F".utf8)
        case .pageUp:
            return Array("\u{1b}[5~".utf8)
        case .pageDown:
            return Array("\u{1b}[6~".utf8)
        case let .function(number):
            let codes = [11, 12, 13, 14, 15, 17, 18, 19, 20, 21, 23, 24]
            guard number >= 1, number <= codes.count else { return nil }
            return Array("\u{1b}[\(codes[number - 1])~".utf8)
        case .character:
            return nil
        }
    }
}
