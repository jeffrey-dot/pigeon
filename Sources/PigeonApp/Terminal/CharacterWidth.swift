/// Grid columns a scalar takes, for text pigeon draws itself (the input method's preedit).
/// Cells from the host already carry their width; this is only for strings that have not
/// been through the terminal yet, so a close approximation of `wcwidth` is enough.
enum CharacterWidth {
    static func of(_ scalar: Unicode.Scalar) -> Int {
        let value = scalar.value
        if value < 0x20 || (0x7f..<0xa0).contains(value) { return 0 }
        switch scalar.properties.generalCategory {
        case .nonspacingMark, .enclosingMark, .format: return 0
        default: break
        }
        if scalar.properties.isEmojiPresentation { return 2 }
        return wide.contains { $0.contains(value) } ? 2 : 1
    }

    /// East Asian Wide and Fullwidth blocks (Unicode 15, coarse).
    private static let wide: [ClosedRange<UInt32>] = [
        0x1100...0x115f, 0x2e80...0x303e, 0x3041...0x33ff, 0x3400...0x4dbf,
        0x4e00...0x9fff, 0xa000...0xa4cf, 0xa960...0xa97f, 0xac00...0xd7a3,
        0xf900...0xfaff, 0xfe10...0xfe19, 0xfe30...0xfe6f, 0xff00...0xff60,
        0xffe0...0xffe6, 0x1b000...0x1b2ff, 0x20000...0x2fffd, 0x30000...0x3fffd,
    ]
}
