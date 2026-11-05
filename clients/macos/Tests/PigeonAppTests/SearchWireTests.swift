import XCTest
import PigeonCore
@testable import PigeonApp

final class SearchWireTests: XCTestCase {
    func testRequestAndLocatedPictureUseRealABIAndOwnTheirMemory() throws {
        let core = try XCTUnwrap(pg_core_new(nil))
        defer { pg_core_free(core) }
        let pane = pg_pane_new(core, nil)
        let bytes = Array("你好é\r\n".utf8)
        XCTAssertTrue(bytes.withUnsafeBufferPointer { pg_pane_feed(core, pane, $0.baseAddress, $0.count) })
        var query = PgTextQuery()
        query.size = UInt32(MemoryLayout<PgTextQuery>.size)
        query.version = UInt32(PG_CORE_ABI_VERSION)
        query.kind = 1; query.max_results = 128; query.case_sensitive = true
        let pattern = Array("你好é".utf8)
        let result = try XCTUnwrap(pattern.withUnsafeBufferPointer { text -> OpaquePointer? in
            query.pattern = PgStr(size: UInt32(MemoryLayout<PgStr>.size), ptr: text.baseAddress, len: text.count)
            return pg_text_query(core, pane, &query)
        })
        let page = try XCTUnwrap(SearchWire.page(result))
        pg_text_result_free(result)
        XCTAssertEqual(page.hits.count, 1)
        let context = TerminalSearch.Context(host: "local", instance: "one", connection: UUID(), pane: pane)
        var model = TerminalSearch(context: context)
        model.begin(pattern: "你好é", caseSensitive: true)
        model.requestPage(id: 7)
        XCTAssertTrue(model.receivePage(id: 7, context: context, generation: page.generation, hits: page.hits, next: nil))
        let range = try XCTUnwrap(model.select(step: 1))
        let request = try XCTUnwrap(SearchWire.request(id: 42, model: model, range: range))
        defer { pg_msg_free(request) }
        let frame = try XCTUnwrap(Wire.frame(request))
        var consumed = 0
        let decoded = try XCTUnwrap(frame.withUnsafeBytes { bytes -> OpaquePointer? in
            pg_msg_decode_frame(bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, &consumed)
        })
        defer { pg_msg_free(decoded) }
        XCTAssertEqual(Wire.type(decoded), PG_MSG_TEXT_QUERY)
        XCTAssertEqual(Wire.u32(decoded, "id"), 42)
        // The same typed range can be queried in embedded mode without reimplementing CBOR.
        query.kind = 3; query.generation = page.generation
        query.range = PgTextRange(size: UInt32(MemoryLayout<PgTextRange>.size),
                                 start_row: range.start.row, start_col: range.start.col, start_byte: range.start.byte,
                                 end_row: range.end.row, end_col: range.end.col, end_byte: range.end.byte)
        let located = try XCTUnwrap(pg_text_query(core, pane, &query))
        let info = try XCTUnwrap(SearchWire.info(located))
        XCTAssertEqual(info.kind, 3)
        let viewport = try XCTUnwrap(SearchWire.viewport(located))
        XCTAssertEqual(viewport.offset, 0)
        XCTAssertEqual(viewport.history_len, 0)
        XCTAssertEqual(SearchWire.range(located, index: 0), range)
        let snapshot = try XCTUnwrap(pg_text_result_snapshot(located))
        pg_text_result_free(located)
        defer { pg_snapshot_free(snapshot) }
        XCTAssertEqual(snapshot.pointee.history_count, 0)
        XCTAssertEqual(snapshot.pointee.screen?.pointee.cells?.pointee.codepoint, 0x4F60)
        XCTAssertGreaterThan(snapshot.pointee.text_count, 0)
    }
}
