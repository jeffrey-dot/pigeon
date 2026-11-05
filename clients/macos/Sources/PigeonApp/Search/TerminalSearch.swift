import Foundation

struct SearchPoint: Equatable {
    var row: Int32
    var col: UInt16
    var byte: UInt16
}

struct SearchRange: Equatable {
    var start: SearchPoint
    var end: SearchPoint

    /// Byte offsets select the containing cell; a wide cell includes its continuation.
    func spans(topRow: Int32, cols: Int, rows: Int, widths: [UInt8]) -> [SelectionSpan] {
        guard cols > 0, rows > 0 else { return [] }
        let firstRow = max(Int(start.row) - Int(topRow), 0)
        let lastRow = min(Int(end.row) - Int(topRow), rows - 1)
        guard firstRow <= lastRow else { return [] }
        return (firstRow...lastRow).compactMap { row in
            let sourceRow = Int(topRow) + row
            let first = sourceRow == Int(start.row) ? Int(start.col) : 0
            var endColumn = sourceRow == Int(end.row) ? Int(end.col) : cols
            if sourceRow == Int(end.row), end.byte > 0, endColumn < cols {
                let index = row * cols + endColumn
                endColumn += index < widths.count ? max(Int(widths[index]), 1) : 1
            }
            let last = min(endColumn, cols) - 1
            guard first <= last, first < cols else { return nil }
            return SelectionSpan(row: row, first: first, last: last)
        }
    }
}

/// One search belongs to one transport and pane, independent of mutable AppDelegate routing.
struct TerminalSearch {
    struct Context: Equatable {
        var host: String
        var instance: String
        var connection: UUID
        var pane: UInt32
    }
    let context: Context
    private(set) var pattern = ""
    private(set) var caseSensitive = true
    private(set) var generation: UInt64 = 0
    private(set) var hits: [SearchRange] = []
    private(set) var next: SearchPoint?
    private(set) var selected: Int?
    private(set) var scanning = false
    private(set) var limited = false
    private(set) var notice = "Literal · Entire pane history and screen"
    private(set) var pageRequest: UInt32?
    private(set) var locationRequest: UInt32?
    private(set) var frozen = false
    static let maxHits = 10_000
    var canCancel: Bool { scanning || pageRequest != nil || locationRequest != nil }

    mutating func begin(pattern: String, caseSensitive: Bool) {
        self.pattern = pattern
        self.caseSensitive = caseSensitive
        generation = 0; hits = []; next = nil; selected = nil; limited = false
        pageRequest = nil; locationRequest = nil
        scanning = !pattern.isEmpty && pattern.utf8.count <= 256
        notice = scanning ? "Scanning…" : (pattern.isEmpty ? "Enter literal text" : "Maximum 256 UTF-8 bytes")
    }
    mutating func requestPage(id: UInt32) { pageRequest = id }
    mutating func requestLocation(id: UInt32) { locationRequest = id }

    mutating func receivePage(id: UInt32, context: Context, generation: UInt64,
                              hits page: [SearchRange], next: SearchPoint?) -> Bool {
        guard context == self.context, id == pageRequest, scanning else { return false }
        pageRequest = nil
        guard self.generation == 0 || self.generation == generation else { expire(); return false }
        self.generation = generation
        limited = page.count > Self.maxHits - hits.count
        hits.append(contentsOf: page.prefix(Self.maxHits - hits.count))
        self.next = next
        limited = limited || (next != nil && hits.count == Self.maxHits)
        scanning = next != nil && !limited
        notice = scanning ? "Scanning…" : (limited ? "Limit reached — refine search" : "Complete")
        return true
    }
    mutating func select(step: Int) -> SearchRange? {
        guard !hits.isEmpty, generation != 0 else { return nil }
        let index = selected.map { ($0 + step + hits.count) % hits.count } ?? (step > 0 ? 0 : hits.count - 1)
        selected = index
        locationRequest = nil
        return hits[index]
    }
    var selectedRange: SearchRange? { selected.flatMap { hits.indices.contains($0) ? hits[$0] : nil } }
    mutating func receiveLocation(id: UInt32, context: Context, generation: UInt64, range: SearchRange) -> Bool {
        guard context == self.context, id == locationRequest,
              generation == self.generation, range == selectedRange else { return false }
        locationRequest = nil
        frozen = true
        return true
    }
    mutating func fail(id: UInt32, context: Context, code: String) -> Bool {
        guard context == self.context, id == pageRequest || id == locationRequest else { return false }
        if code == "stale_text" { expire() }
        else { cancel(); notice = code == "limit" ? "Query limit — refine search" : "Search unavailable (\(code))" }
        return true
    }
    mutating func cancel() {
        scanning = false; pageRequest = nil; locationRequest = nil
        notice = "Cancelled · Partial results"
    }
    mutating func expire() {
        scanning = false; pageRequest = nil; locationRequest = nil
        hits = []; selected = nil; next = nil; generation = 0
        notice = "Expired — Search Again"
    }
    var summary: String {
        let position = selected.map { "\($0 + 1) / " } ?? ""
        let suffix = next != nil || limited ? "+" : ""
        return "\(position)\(hits.count)\(suffix) matches · \(notice)"
    }
}
