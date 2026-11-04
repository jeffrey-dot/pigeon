/// The client-owned copy of a core cell, also used by pure selection tests.
struct TerminalCell: Equatable, Sendable {
    var codepoint: UInt32
    var attrs: UInt8
    var fg: UInt32
    var bg: UInt32
    var width: UInt8

    static let blank = TerminalCell(codepoint: 32, attrs: 0, fg: 0, bg: 0, width: 1)
}
