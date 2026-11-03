import Foundation

/// Whether a blinking cursor is lit right now. Pure (time is passed in): the cursor is solid
/// while the user types and for `rest` after, then alternates every `period`, so it never
/// vanishes under the character just typed.
struct CursorBlink: Equatable {
    static let period: TimeInterval = 0.53
    static let rest: TimeInterval = 0.5

    private(set) var lastInput: Date

    init(now: Date) {
        lastInput = now
    }

    mutating func typed(at now: Date) {
        lastInput = now
    }

    func isLit(at now: Date) -> Bool {
        let idle = now.timeIntervalSince(lastInput) - Self.rest
        // A clock that went backwards, or a fresh keystroke: solid.
        guard idle > 0 else { return true }
        return Int(idle / Self.period) % 2 == 1
    }
}
