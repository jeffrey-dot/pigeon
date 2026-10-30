/// Rows whose instances must be rebuilt before the next frame. Deltas name rows; a snapshot,
/// a resize or a palette switch invalidates everything.
struct DirtyRows: Equatable, Sendable {
    private(set) var all: Bool
    private(set) var rows: Set<Int>

    static let none = DirtyRows(all: false, rows: [])
    static let everything = DirtyRows(all: true, rows: [])

    var isEmpty: Bool { !all && rows.isEmpty }

    mutating func mark(_ row: Int) {
        guard !all else { return }
        rows.insert(row)
    }

    mutating func markAll() {
        all = true
        rows = []
    }

    /// Frames coalesce when the render thread falls behind: nothing marked by either is lost.
    func union(_ other: DirtyRows) -> DirtyRows {
        if all || other.all { return .everything }
        return DirtyRows(all: false, rows: rows.union(other.rows))
    }

    func contains(_ row: Int) -> Bool { all || rows.contains(row) }
}
