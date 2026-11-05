import Foundation
import PigeonCore

/// Typed C ABI adapter; protocol framing and validation remain in core.
enum SearchWire {
    struct Page {
        var id: UInt32
        var pane: UInt32
        var generation: UInt64
        var hits: [SearchRange]
        var next: SearchPoint?
    }
    static func request(id: UInt32, model: TerminalSearch, range: SearchRange? = nil) -> OpaquePointer? {
        var query = PgTextQuery()
        query.size = UInt32(MemoryLayout<PgTextQuery>.size)
        query.version = UInt32(PG_CORE_ABI_VERSION)
        query.kind = range == nil ? 1 : 3
        query.generation = model.generation
        query.case_sensitive = model.caseSensitive
        query.max_results = 128
        query.range.size = UInt32(MemoryLayout<PgTextRange>.size)
        if let range { query.range = wireRange(range) }
        else if let point = model.next {
            query.start_set = true
            query.range = wireRange(SearchRange(start: point, end: point))
        }
        return Array(model.pattern.utf8).withUnsafeBufferPointer { bytes in
            query.pattern = PgStr(size: UInt32(MemoryLayout<PgStr>.size), ptr: bytes.baseAddress, len: bytes.count)
            return pg_msg_text_query(id, model.context.pane, &query)
        }
    }
    static func info(_ result: OpaquePointer) -> PgTextResultInfo? {
        var info = PgTextResultInfo()
        info.size = UInt32(MemoryLayout<PgTextResultInfo>.size)
        return pg_text_result_info(result, &info) ? info : nil
    }
    static func viewport(_ result: OpaquePointer) -> PgTextViewportInfo? {
        var info = PgTextViewportInfo()
        info.size = UInt32(MemoryLayout<PgTextViewportInfo>.size)
        return pg_text_result_viewport(result, &info) ? info : nil
    }
    static func range(_ result: OpaquePointer, index: Int) -> SearchRange? {
        var range = PgTextRange()
        range.size = UInt32(MemoryLayout<PgTextRange>.size)
        guard pg_text_result_hit(result, index, &range) else { return nil }
        return SearchRange(start: .init(row: range.start_row, col: range.start_col, byte: range.start_byte),
                           end: .init(row: range.end_row, col: range.end_col, byte: range.end_byte))
    }
    static func page(_ result: OpaquePointer) -> Page? {
        guard let info = info(result), info.kind == 1 else { return nil }
        var hits: [SearchRange] = []
        for index in 0..<info.count {
            guard let hit = range(result, index: index) else { return nil }
            hits.append(hit)
        }
        return Page(id: info.id, pane: info.pane_id, generation: info.generation, hits: hits,
                    next: info.has_next ? SearchPoint(row: info.next_row, col: info.next_col, byte: info.next_byte) : nil)
    }
    private static func wireRange(_ range: SearchRange) -> PgTextRange {
        PgTextRange(size: UInt32(MemoryLayout<PgTextRange>.size),
                    start_row: range.start.row, start_col: range.start.col, start_byte: range.start.byte,
                    end_row: range.end.row, end_col: range.end.col, end_byte: range.end.byte)
    }
}
