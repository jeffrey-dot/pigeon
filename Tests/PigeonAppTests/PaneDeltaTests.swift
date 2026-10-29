import PigeonCore
import XCTest
@testable import PigeonApp

/// A `delta` patches only the rows it names; a frame that does not fit the grid is refused
/// so the window re-attaches instead of drawing a torn screen.
final class PaneDeltaTests: XCTestCase {
    private func cell(_ character: Character) -> PgCell {
        PgCell(
            size: UInt32(MemoryLayout<PgCell>.size),
            codepoint: character.unicodeScalars.first!.value,
            fg: 0, bg: 0, attrs: 0, hyperlink_id: 0, width: 1)
    }

    private func cursor(x: UInt16, y: UInt16) -> PgCursor {
        PgCursor(size: UInt32(MemoryLayout<PgCursor>.size), x: x, y: y, visible: 1, shape: 0)
    }

    /// A 3×2 pane showing `abc` / `def`.
    private func pane() -> PaneView {
        let view = PaneView(frame: .zero)
        var cells = "abcdef".map(cell)
        cells.withUnsafeMutableBufferPointer { buffer in
            var rows = [
                PgRow(size: UInt32(MemoryLayout<PgRow>.size), cells: UnsafePointer(buffer.baseAddress!)),
                PgRow(size: UInt32(MemoryLayout<PgRow>.size), cells: UnsafePointer(buffer.baseAddress! + 3)),
            ]
            rows.withUnsafeBufferPointer { rowBuffer in
                var snapshot = PgSnapshot(
                    size: UInt32(MemoryLayout<PgSnapshot>.size), version: 1, pane_id: 1,
                    cols: 3, rows: 2, alt_screen: 0, cursor: cursor(x: 0, y: 0),
                    scroll_region: PgScrollRegion(
                        size: UInt32(MemoryLayout<PgScrollRegion>.size), top: 0, bottom: 1),
                    screen: rowBuffer.baseAddress, history: nil, history_count: 0)
                view.apply(snapshot: &snapshot)
            }
        }
        return view
    }

    private func apply(_ text: String, y: UInt16, to view: PaneView, alt: UInt8 = 2) -> Bool {
        var cells = text.map(cell)
        return cells.withUnsafeMutableBufferPointer { buffer in
            var row = PgDeltaRow(
                size: UInt32(MemoryLayout<PgDeltaRow>.size), y: y, cols: UInt16(text.count),
                cells: UnsafePointer(buffer.baseAddress!))
            return withUnsafePointer(to: &row) { rowPointer in
                var delta = PgDelta(
                    size: UInt32(MemoryLayout<PgDelta>.size), pane_id: 1, seq: 1, row_count: 1,
                    rows: rowPointer, has_cursor: 1, cursor: cursor(x: 2, y: y), alt_screen: alt)
                return view.apply(delta: &delta)
            }
        }
    }

    private func text(_ view: PaneView) -> String {
        String(view.cells.map { Character(Unicode.Scalar($0.codepoint)!) })
    }

    func testADeltaReplacesOnlyItsRowAndMovesTheCursor() {
        let view = pane()
        XCTAssertTrue(apply("xyz", y: 1, to: view, alt: 1))
        XCTAssertEqual(text(view), "abcxyz")
        XCTAssertEqual(view.cursor.x, 2)
        XCTAssertEqual(view.cursor.y, 1)
        XCTAssertTrue(view.alt, "the delta carried the alternate screen")
        XCTAssertTrue(apply("uvw", y: 0, to: view))
        XCTAssertTrue(view.alt, "an unreported alt flag keeps the last one")
    }

    func testADeltaThatDoesNotFitTheGridIsRefused() {
        let view = pane()
        XCTAssertFalse(apply("wider", y: 0, to: view), "the host is on another width")
        XCTAssertFalse(apply("xyz", y: 2, to: view), "row outside the grid")
        XCTAssertEqual(text(view), "abcdef", "a refused frame leaves the screen alone")
        XCTAssertFalse(apply("xyz", y: 0, to: PaneView(frame: .zero)), "no snapshot yet")
    }
}
