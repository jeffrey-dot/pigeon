/// The instances one grid row turns into. Kept per row so a delta rebuilds only the rows it
/// names; the rest of the frame is reused as is.
struct RowInstances: Equatable {
    /// Cell backgrounds that differ from the theme background (the clear colour covers it).
    var backgrounds: [RectInstance] = []
    var glyphs: [GlyphInstance] = []
    /// Underline and strikethrough strokes, drawn above the glyphs.
    var decorations: [RectInstance] = []

    /// Builds row `row` of `frame`. `glyph` returns the atlas slot for a key, rasterizing on
    /// first use; nil means the glyph cannot be shown (blank, unrenderable, atlas full).
    /// `text` maps a resolved fg to the colour glyphs are drawn in (the contrast floor).
    /// Shapes a run of cells (text, bold, italic) into one glyph ID per cell, or nil when the
    /// run draws as its characters (`CoreTextRenderer.shapeLigatures`).
    typealias Shaper = (String, Bool, Bool) -> [UInt16]?

    static func build(
        row: Int,
        of frame: GridFrame,
        glyph: (GlyphKey) -> AtlasSlot?,
        shape: Shaper? = nil,
        text textColor: (UInt32) -> UInt32
    ) -> RowInstances {
        var out = RowInstances()
        let metrics = frame.metrics
        let cols = frame.cols
        let start = row * cols
        guard row < frame.rows, start + cols <= frame.cells.count else { return out }
        let top = metrics.rowY(row)
        let bottom = metrics.rowY(row + 1)
        let pixelHeight = UInt16(clamping: Int(metrics.cellHeight.rounded(.up)))
        let shaped = shape.map { ligatures(frame.cells[start..<start + cols], shape: $0) } ?? [:]

        for column in 0..<cols {
            let cell = frame.cells[start + column]
            // A wide glyph's continuation cell is painted by the cell before it.
            guard cell.width != 0 else { continue }
            let shapedGlyph = shaped[column] ?? 0
            let span = min(Int(cell.width), cols - column)
            let left = metrics.columnX(column)
            let right = metrics.columnX(column + span)
            let (fg, bg) = frame.palette.colors(fg: cell.fg, bg: cell.bg, attrs: cell.attrs)
            if bg != frame.palette.background {
                out.backgrounds.append(RectInstance(
                    origin: SIMD2(left, top), size: SIMD2(right - left, bottom - top),
                    color: CellPalette.components(bg)))
            }
            // SGR 2 (faint): same hue at reduced opacity, which keeps it readable on both
            // the light and the dark card where mixing toward the background would not.
            let text = CellPalette.components(
                textColor(fg), alpha: cell.attrs & CellAttrs.dim != 0 ? 0.6 : 1)
            if cell.extraText == nil, let strokes = BoxDrawing.rects(
                cell.codepoint, origin: SIMD2(left, top), size: SIMD2(right - left, bottom - top),
                thin: metrics.lineThickness, color: text) {
                out.decorations.append(contentsOf: strokes)
            } else if (cell.codepoint > 32 || cell.extraText != nil), cell.codepoint != 0x7f, shapedGlyph != GlyphKey.noGlyph {
                let key = GlyphKey(
                    codepoint: cell.codepoint,
                    bold: cell.attrs & CellAttrs.bold != 0,
                    italic: cell.attrs & CellAttrs.italic != 0,
                    span: UInt8(span),
                    pixelHeight: pixelHeight,
                    glyph: shapedGlyph, text: cell.extraText)
                if let slot = glyph(key) {
                    out.glyphs.append(GlyphInstance(
                        origin: SIMD2(left + Float(slot.bearingX), top + Float(slot.bearingY)),
                        size: SIMD2(Float(slot.width), Float(slot.height)),
                        uv: SIMD2(Float(slot.x), Float(slot.y)),
                        colored: slot.colored ? 1 : 0,
                        color: text))
                }
            }
            if cell.attrs & CellAttrs.underline != 0 {
                out.decorations.append(RectInstance(
                    origin: SIMD2(left, top + metrics.baseline + metrics.lineThickness),
                    size: SIMD2(right - left, metrics.lineThickness), color: text))
            }
            if cell.attrs & CellAttrs.strikethrough != 0 {
                out.decorations.append(RectInstance(
                    origin: SIMD2(left, top + metrics.strikeOffset),
                    size: SIMD2(right - left, metrics.lineThickness), color: text))
            }
        }
        return out
    }

    /// Shaped glyph IDs by column for the row's ligature candidates: runs of two or more
    /// ASCII punctuation cells (`->`, `!=`, `===`, `//`) of one style and colour. Letters and
    /// digits are never shaped, so plain text costs nothing extra.
    static func ligatures(_ cells: ArraySlice<PaneView.Cell>, shape: Shaper) -> [Int: UInt16] {
        var out: [Int: UInt16] = [:]
        var runStart: Int?
        func isCandidate(_ cell: PaneView.Cell) -> Bool {
            cell.extraText == nil && cell.width == 1 && cell.codepoint > 0x20 && cell.codepoint < 0x7F
                && !(0x30...0x39).contains(cell.codepoint)
                && !(0x41...0x5A).contains(cell.codepoint) && !(0x61...0x7A).contains(cell.codepoint)
                && !BoxDrawing.handles(cell.codepoint)
        }
        func flush(_ end: Int) {
            guard let first = runStart, end - first >= 2 else { return }
            let run = cells[(cells.startIndex + first)..<(cells.startIndex + end)]
            let text = String(String.UnicodeScalarView(run.compactMap { Unicode.Scalar($0.codepoint) }))
            let head = run[run.startIndex]
            guard let glyphs = shape(text, head.attrs & CellAttrs.bold != 0, head.attrs & CellAttrs.italic != 0),
                  glyphs.count == run.count
            else { return }
            for (offset, glyph) in glyphs.enumerated() { out[first + offset] = glyph }
        }
        var previous: PaneView.Cell?
        for (column, cell) in cells.enumerated() {
            let sameStyle = previous.map { $0.attrs == cell.attrs && $0.fg == cell.fg && $0.bg == cell.bg } ?? false
            if isCandidate(cell) {
                if runStart == nil || !sameStyle {
                    flush(column)
                    runStart = column
                }
            } else {
                flush(column)
                runStart = nil
            }
            previous = cell
        }
        flush(cells.count)
        return out
    }
}
