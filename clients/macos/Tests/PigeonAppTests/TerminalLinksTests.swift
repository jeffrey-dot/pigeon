import XCTest
@testable import PigeonApp

final class TerminalLinksTests: XCTestCase {
    func testOnlyExplicitWebURLsCanOpen() {
        for uri in ["https://example.com/path?q=1#fragment", "http://localhost:8080/a%20b", "HTTPS://example.com"] {
            XCTAssertNotNil(TerminalLinks.openingURL(uri), uri)
        }
        for uri in ["file:///etc/passwd", "javascript:alert(1)", "data:text/html,hi", "mailto:a@example.com",
                    "ssh://example.com", "https:///", "//example.com", "https://user:pass@example.com",
                    "https://example.com/\n", "https://example.com/%0a", "https://example.com/%1b",
                    "https://ex ample.com", "https://example.com/\u{202e}", "https://example.com/%E2%80%AE",
                    "https://example.com/\\evil", "https://example.com/%invalid", "https://example.com/%5c", "https://example.com:99999", String(repeating: "x", count: 2049)] {
            XCTAssertNil(TerminalLinks.openingURL(uri), uri)
        }
    }

    func testRowReplacementDoesNotRetargetOtherCells() {
        var old = TerminalCell.blank
        old.hyperlink = "https://old.example"
        var cells = [old, old]
        var replacement = TerminalCell.blank
        replacement.hyperlink = "https://new.example"
        cells[1] = replacement
        XCTAssertEqual(cells[0].hyperlink, "https://old.example")
        XCTAssertEqual(cells[1].hyperlink, "https://new.example")
        cells[1] = .blank
        XCTAssertNil(cells[1].hyperlink)
    }

    func testLinkPressRemainsCancelledAfterTargetChangesBack() throws {
        let url = try XCTUnwrap(TerminalLinks.openingURL("https://old.example"))
        var press = TerminalLinkPress(url: url, point: .init(row: 0, column: 0))
        var cell = TerminalCell.blank
        cell.hyperlink = "https://old.example"
        press.validate(cells: [cell], columns: 1, rows: 1)
        XCTAssertEqual(press.url, url)
        cell.hyperlink = "https://new.example"
        press.validate(cells: [cell], columns: 1, rows: 1)
        cell.hyperlink = "https://old.example"
        press.validate(cells: [cell], columns: 1, rows: 1)
        XCTAssertNil(press.url)
    }

    func testLinkPressCancelsWhenCellDisappearsOrDragBegins() throws {
        let url = try XCTUnwrap(TerminalLinks.openingURL("https://old.example"))
        var press = TerminalLinkPress(url: url, point: .init(row: 0, column: 1))
        press.validate(cells: [.blank], columns: 1, rows: 1)
        XCTAssertNil(press.url)
        press = TerminalLinkPress(url: url, point: .init(row: 0, column: 0))
        press.cancel()
        var cell = TerminalCell.blank
        cell.hyperlink = "https://old.example"
        press.validate(cells: [cell], columns: 1, rows: 1)
        XCTAssertNil(press.url)
    }
}
