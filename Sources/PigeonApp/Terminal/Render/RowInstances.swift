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
    static func build(
        row: Int,
        of frame: GridFrame,
        glyph: (GlyphKey) -> AtlasSlot?,
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

        for column in 0..<cols {
            let cell = frame.cells[start + column]
            // A wide glyph's continuation cell is painted by the cell before it.
            guard cell.width != 0 else { continue }
            let span = min(Int(cell.width), cols - column)
            let left = metrics.columnX(column)
            let right = metrics.columnX(column + span)
            let (fg, bg) = frame.palette.colors(fg: cell.fg, bg: cell.bg, attrs: cell.attrs)
            if bg != frame.palette.background {
                out.backgrounds.append(RectInstance(
                    origin: SIMD2(left, top), size: SIMD2(right - left, bottom - top),
                    color: CellPalette.components(bg)))
            }
            let text = CellPalette.components(textColor(fg))
            if cell.codepoint > 32, cell.codepoint != 0x7f {
                let key = GlyphKey(
                    codepoint: cell.codepoint,
                    bold: cell.attrs & CellAttrs.bold != 0,
                    italic: cell.attrs & CellAttrs.italic != 0,
                    span: UInt8(span),
                    pixelHeight: pixelHeight)
                if let slot = glyph(key) {
                    out.glyphs.append(GlyphInstance(
                        origin: SIMD2(left, top),
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
}
