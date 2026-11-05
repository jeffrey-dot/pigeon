import XCTest
@testable import PigeonApp

final class TerminalSearchTests: XCTestCase {
    let context = TerminalSearch.Context(host: "local", instance: "one", connection: UUID(), pane: 1)
    let hit = SearchRange(start: .init(row: -5, col: 2, byte: 0), end: .init(row: -5, col: 3, byte: 1))

    func testCancellationAndReplacementRejectLatePagesAndLocations() {
        var model = TerminalSearch(context: context)
        model.begin(pattern: "hello", caseSensitive: false)
        model.requestPage(id: 1)
        model.cancel()
        XCTAssertFalse(model.receivePage(id: 1, context: context, generation: 4, hits: [hit], next: nil))
        model.begin(pattern: "world", caseSensitive: true)
        model.requestPage(id: 2)
        XCTAssertTrue(model.receivePage(id: 2, context: context, generation: 5, hits: [hit], next: nil))
        XCTAssertEqual(model.select(step: 1), hit)
        model.requestLocation(id: 3)
        model.begin(pattern: "new", caseSensitive: true)
        XCTAssertFalse(model.receiveLocation(id: 3, context: context, generation: 5, range: hit))
    }

    func testIdentityIncludesHostInstanceConnectionAndPane() {
        var model = TerminalSearch(context: context)
        model.begin(pattern: "hello", caseSensitive: true)
        model.requestPage(id: 1)
        for other in [TerminalSearch.Context(host: "other", instance: "one", connection: context.connection, pane: 1),
                      .init(host: "local", instance: "two", connection: context.connection, pane: 1),
                      .init(host: "local", instance: "one", connection: UUID(), pane: 1),
                      .init(host: "local", instance: "one", connection: context.connection, pane: 2)] {
            XCTAssertFalse(model.receivePage(id: 1, context: other, generation: 4, hits: [hit], next: nil))
        }
        XCTAssertTrue(model.receivePage(id: 1, context: context, generation: 4, hits: [], next: hit.start))
        XCTAssertTrue(model.scanning)
        model.requestPage(id: 2)
        XCTAssertTrue(model.receivePage(id: 2, context: context, generation: 4, hits: [hit], next: nil))
        XCTAssertEqual(model.select(step: -1), hit)
        XCTAssertFalse(model.scanning)
    }

    func testStopRemainsAvailableForLocationAfterLastPage() {
        var model = TerminalSearch(context: context)
        model.begin(pattern: "hello", caseSensitive: true)
        model.requestPage(id: 1)
        XCTAssertTrue(model.receivePage(id: 1, context: context, generation: 4, hits: [hit], next: nil))
        XCTAssertFalse(model.canCancel)
        XCTAssertEqual(model.select(step: 1), hit)
        model.requestLocation(id: 2)
        XCTAssertTrue(model.canCancel)
        model.cancel()
        XCTAssertFalse(model.canCancel)
        XCTAssertFalse(model.receiveLocation(id: 2, context: context, generation: 4, range: hit))
    }

    func testStalePagesDiscardCoordinatesAndErrorsIgnoreOldRequests() {
        var model = TerminalSearch(context: context)
        model.begin(pattern: "hello", caseSensitive: true)
        model.requestPage(id: 1)
        XCTAssertTrue(model.receivePage(id: 1, context: context, generation: 4, hits: [hit], next: hit.end))
        model.requestPage(id: 2)
        XCTAssertFalse(model.fail(id: 1, context: context, code: "stale_text"))
        XCTAssertTrue(model.fail(id: 2, context: context, code: "stale_text"))
        XCTAssertTrue(model.hits.isEmpty)
        XCTAssertNil(model.select(step: 1))
        XCTAssertEqual(model.notice, "Expired — Search Again")
    }

    func testResultLimitDoesNotClaimAnExactCount() {
        var model = TerminalSearch(context: context)
        model.begin(pattern: "x", caseSensitive: true)
        model.requestPage(id: 1)
        XCTAssertTrue(model.receivePage(id: 1, context: context, generation: 4,
                                        hits: Array(repeating: hit, count: TerminalSearch.maxHits + 1), next: nil))
        XCTAssertFalse(model.scanning)
        XCTAssertTrue(model.limited)
        XCTAssertEqual(model.hits.count, TerminalSearch.maxHits)
        XCTAssertTrue(model.summary.contains("10000+"))
    }

    func testHighlightClipsRowsAndIncludesWideAndCombiningCells() {
        let range = SearchRange(start: .init(row: -5, col: 2, byte: 1), end: .init(row: -4, col: 1, byte: 2))
        let spans = range.spans(topRow: -5, cols: 6, rows: 2, widths: [1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 1, 1])
        XCTAssertEqual(spans.map { [$0.row, $0.first, $0.last] }, [[0, 2, 5], [1, 0, 1]])
    }
}
