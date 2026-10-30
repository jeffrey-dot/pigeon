/// `PG_ATTR_*` bits as `PaneView.Cell` stores them (truncated to 8 bits).
enum CellAttrs {
    static let bold: UInt8 = 1 << 0
    static let italic: UInt8 = 1 << 1
    static let underline: UInt8 = 1 << 2
    static let reverse: UInt8 = 1 << 3
    static let strikethrough: UInt8 = 1 << 4
    static let dim: UInt8 = 1 << 5
}
