/// The `MODE_*` bits a pane's program turned on, as the host forwards them in `snapshot` and
/// `delta` (`modes`, `docs/protocol.md` §3.3; mirrors `core/src/term/mod.rs`).
enum TerminalModes {
    static let appCursor: UInt32 = 1 << 0
    static let bracketedPaste: UInt32 = 1 << 1
    static let mouseClick: UInt32 = 1 << 2
    static let mouseDrag: UInt32 = 1 << 3
    static let mouseMotion: UInt32 = 1 << 4
    static let mouseSGR: UInt32 = 1 << 5
    static let focusEvents: UInt32 = 1 << 6
    static let appKeypad: UInt32 = 1 << 7
    static let alternateScroll: UInt32 = 1 << 8

    /// Any of the three mouse-reporting modes: clicks then belong to the program.
    static let anyMouse = mouseClick | mouseDrag | mouseMotion
}
