import XCTest
import PigeonCore
@testable import PigeonApp

final class TerminalLinksWireTests: XCTestCase {
    func testCoreLinksAreCopiedBeforeSnapshotAndCoreAreFreed() throws {
        let core = try XCTUnwrap(pg_core_new(nil))
        let pane = pg_pane_new(core, nil)
        let input = Array("\u{1b}]8;;https://example.com/owned\u{7}hi\u{1b}]8;;\u{7}!".utf8)
        XCTAssertTrue(input.withUnsafeBufferPointer { pg_pane_feed(core, pane, $0.baseAddress, $0.count) })
        let snapshot = try XCTUnwrap(pg_pane_snapshot(core, pane))
        pg_core_free(core)
        let frame = snapshot.pointee
        let links = TerminalLinks.read(frame.links, count: frame.link_count)
        let source = try XCTUnwrap(frame.screen?.pointee.cells)
        let copied = (0..<3).map { column -> TerminalCell in
            TerminalCell(source[column], links: links)
        }
        let unknown = TerminalCell(source[0], links: [:])
        XCTAssertNil(unknown.hyperlink)
        pg_snapshot_free(snapshot)
        XCTAssertEqual(copied[0].hyperlink, "https://example.com/owned")
        XCTAssertEqual(copied[1].hyperlink, copied[0].hyperlink)
        XCTAssertNil(copied[2].hyperlink)
        XCTAssertEqual(TerminalLinks.openingURL(copied[0].hyperlink)?.host, "example.com")
    }
}
