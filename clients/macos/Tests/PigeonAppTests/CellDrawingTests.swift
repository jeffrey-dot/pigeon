import XCTest
@testable import PigeonApp

/// Cursor shapes, the faint attribute and procedural box drawing: what a row or the
/// overlay turns into, without a GPU.
final class CellDrawingTests: XCTestCase {
    private let metrics = GridMetrics(
        cellWidth: 16, cellHeight: 32, baseline: 24, lineThickness: 2, strikeOffset: 16, scale: 2)
    private let palette = CellPalette(
        ansi: Array(repeating: 0x123456, count: 16), foreground: 0x111111, background: 0xffffff)

    private func frame(_ cells: [PaneView.Cell], cols: Int, cursor: PaneView.Cursor) -> GridFrame {
        GridFrame(
            cells: cells, cols: cols, rows: cells.count / cols, cursor: cursor, palette: palette,
            metrics: metrics, width: 100, height: 100, thumb: nil,
            thumbColor: SIMD4(0, 0, 0, 0), dirty: .everything)
    }

    private func blanks(_ count: Int) -> [PaneView.Cell] { Array(repeating: .blank, count: count) }

    func testCursorShapes() {
        var cursor = PaneView.Cursor(x: 1, y: 0, visible: true, shape: .block)
        let block = GridInstances.cursorRects(for: frame(blanks(4), cols: 4, cursor: cursor))
        XCTAssertEqual(block.map(\.size), [SIMD2(16, 32)])

        cursor.shape = .bar
        let bar = GridInstances.cursorRects(for: frame(blanks(4), cols: 4, cursor: cursor))
        XCTAssertEqual(bar.map(\.size), [SIMD2(4, 32)])
        XCTAssertEqual(bar.first?.origin, SIMD2(16, 0))

        cursor.shape = .underline
        let line = GridInstances.cursorRects(for: frame(blanks(4), cols: 4, cursor: cursor))
        XCTAssertEqual(line.first?.origin, SIMD2(16, 28))

        cursor.hollow = true
        XCTAssertEqual(GridInstances.cursorRects(for: frame(blanks(4), cols: 4, cursor: cursor)).count, 4)
    }

    func testTheCursorCoversBothHalvesOfAWideCharacter() {
        var cells = blanks(4)
        cells[0] = PaneView.Cell(codepoint: 0x4e2d, attrs: 0, fg: 0, bg: 0, width: 2)
        cells[1] = PaneView.Cell(codepoint: 0, attrs: 0, fg: 0, bg: 0, width: 0)
        let rects = GridInstances.cursorRects(
            for: frame(cells, cols: 4, cursor: .init(x: 0, y: 0, visible: true)))
        XCTAssertEqual(rects.first?.size, SIMD2(32, 32))
    }

    func testAnUnknownShapeFromTheWireIsABlock() {
        XCTAssertNil(PaneView.Cursor.Shape(rawValue: 9))
        XCTAssertEqual(PaneView.Cursor.Shape(rawValue: 9) ?? .block, .block)
    }

    func testFaintTextIsDrawnTranslucent() {
        let cells = [PaneView.Cell(codepoint: 65, attrs: CellAttrs.dim, fg: 0, bg: 0, width: 1)]
        let row = RowInstances.build(
            row: 0, of: frame(cells, cols: 1, cursor: .init(x: 0, y: 0, visible: false)),
            glyph: { _ in AtlasSlot(x: 0, y: 0, width: 16, height: 32, colored: false) },
            text: { $0 })
        XCTAssertEqual(row.glyphs.first?.color.w, 0.6)
    }

    func testBoxLinesSpanTheWholeCellSoNeighboursJoin() throws {
        let origin = SIMD2<Float>(32, 64)
        let size = SIMD2<Float>(16, 32)
        let horizontal = try XCTUnwrap(BoxDrawing.rects(0x2500, origin: origin, size: size, thin: 2, color: .one))
        let left = horizontal.map(\.origin.x).min()
        let right = horizontal.map { $0.origin.x + $0.size.x }.max()
        XCTAssertEqual(left, 32)
        XCTAssertEqual(right, 48)

        let vertical = try XCTUnwrap(BoxDrawing.rects(0x2502, origin: origin, size: size, thin: 2, color: .one))
        XCTAssertEqual(vertical.map(\.origin.y).min(), 64)
        XCTAssertEqual(vertical.map { $0.origin.y + $0.size.y }.max(), 96)

        // A corner has exactly its two arms; a double line draws two strokes per arm.
        XCTAssertEqual(BoxDrawing.rects(0x250C, origin: origin, size: size, thin: 2, color: .one)?.count, 2)
        XCTAssertEqual(BoxDrawing.rects(0x2550, origin: origin, size: size, thin: 2, color: .one)?.count, 4)
    }

    func testBlocksAndShades() throws {
        let full = try XCTUnwrap(BoxDrawing.rects(0x2588, origin: .zero, size: SIMD2(16, 32), thin: 2, color: .one))
        XCTAssertEqual(full.first?.size, SIMD2(16, 32))
        let lower = try XCTUnwrap(BoxDrawing.rects(0x2584, origin: .zero, size: SIMD2(16, 32), thin: 2, color: .one))
        XCTAssertEqual(lower.first?.origin.y, 16)
        let shade = try XCTUnwrap(BoxDrawing.rects(0x2592, origin: .zero, size: SIMD2(16, 32), thin: 2, color: .one))
        XCTAssertEqual(shade.first?.color.w, 0.5)
    }

    func testOrdinaryCharactersStayWithTheFont() {
        XCTAssertNil(BoxDrawing.rects(65, origin: .zero, size: SIMD2(16, 32), thin: 2, color: .one))
        XCTAssertNil(BoxDrawing.rects(0x2594, origin: .zero, size: SIMD2(16, 32), thin: 2, color: .one))
        XCTAssertFalse(BoxDrawing.handles(0x4e2d))
        XCTAssertTrue(BoxDrawing.handles(0x2500))
    }

    func testABoxRowDrawsNoFontGlyph() {
        let cells = [PaneView.Cell(codepoint: 0x2500, attrs: 0, fg: 0, bg: 0, width: 1)]
        var looked = false
        let row = RowInstances.build(
            row: 0, of: frame(cells, cols: 1, cursor: .init(x: 0, y: 0, visible: false)),
            glyph: { _ in looked = true; return nil }, text: { $0 })
        XCTAssertFalse(looked)
        XCTAssertTrue(row.glyphs.isEmpty)
        XCTAssertFalse(row.decorations.isEmpty)
    }
}
