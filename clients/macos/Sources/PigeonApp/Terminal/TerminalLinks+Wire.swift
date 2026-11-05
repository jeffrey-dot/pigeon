import PigeonCore

extension TerminalLinks {
    /// Copy while the frame owns the bytes. Never cache a table across frames: replacing
    /// a row must also replace its links, while untouched cells keep their own URI.
    static func read(_ entries: UnsafePointer<PgLink>?, count: UInt32) -> [UInt32: String] {
        guard let entries, count <= 1024 else { return [:] }
        var result: [UInt32: String] = [:]
        for index in 0..<Int(count) {
            let entry = entries[index]
            guard entry.id != 0, result[entry.id] == nil else { return [:] }
            result[entry.id] = Wire.string(entry.uri)
        }
        return result
    }
}

extension TerminalCell {
    init(_ cell: PgCell, links: [UInt32: String]) {
        self.init(codepoint: cell.codepoint, attrs: UInt8(truncatingIfNeeded: cell.attrs),
                  fg: cell.fg, bg: cell.bg, width: cell.width,
                  hyperlink: links[cell.hyperlink_id])
    }
}
