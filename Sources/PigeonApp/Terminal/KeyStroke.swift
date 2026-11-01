import Foundation

/// One key event, already decoded from AppKit. Pure data so it can be tested
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
        /// A modifier key on its own; only the kitty protocol's "report all keys" sends these.
        case modifier(Modifier)
    }

    public enum Modifier: Equatable {
        case leftShift, leftControl, leftOption, leftCommand
        case rightShift, rightControl, rightOption, rightCommand
        case capsLock
    }

    /// Press, auto-repeat or release (the kitty protocol's event types, flag 2).
    public enum Action: Equatable {
        case press, repeated, release
    }

    public var key: Key
    public var control: Bool
    public var option: Bool
    public var shift: Bool
    public var command: Bool
    public var capsLock: Bool
    /// The key sits on the numeric keypad (DECKPAM and the kitty `KP_*` codes care).
    public var keypad: Bool
    public var action: Action
    /// The same physical key on the ASCII-capable layout, when `key` is a character the
    /// current layout makes non-ASCII (`c` for Russian `с`). Ctrl chords and the kitty
    /// protocol's base-layout key need it; nil when unknown or not needed.
    public var base: Character?
    /// What the key types with no modifiers on the current layout (`1` for Shift+1's `!`);
    /// nil means "the lowercase of `key`".
    public var unshifted: Character?
    /// The text the key would insert, for the kitty protocol's associated text (flag 16).
    public var text: String?

    public var isModifierKey: Bool {
        if case .modifier = key { return true }
        return false
    }

    public init(
        key: Key,
        control: Bool = false,
        option: Bool = false,
        shift: Bool = false,
        command: Bool = false,
        capsLock: Bool = false,
        keypad: Bool = false,
        action: Action = .press,
        base: Character? = nil,
        unshifted: Character? = nil,
        text: String? = nil
    ) {
        self.key = key
        self.control = control
        self.option = option
        self.shift = shift
        self.command = command
        self.capsLock = capsLock
        self.keypad = keypad
        self.action = action
        self.base = base
        self.unshifted = unshifted
        self.text = text
    }
}
