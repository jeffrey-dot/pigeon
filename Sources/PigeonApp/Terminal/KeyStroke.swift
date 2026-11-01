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
        case insert
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
    /// The same physical key on the ASCII-capable layout, when `key` is a character the
    /// current layout makes non-ASCII (`c` for Russian `с`). Ctrl chords and the kitty
    /// protocol's base-layout key need it; nil when unknown or not needed.
    public var base: Character?

    public init(
        key: Key,
        control: Bool = false,
        option: Bool = false,
        shift: Bool = false,
        command: Bool = false,
        base: Character? = nil
    ) {
        self.key = key
        self.control = control
        self.option = option
        self.shift = shift
        self.command = command
        self.base = base
    }
}
