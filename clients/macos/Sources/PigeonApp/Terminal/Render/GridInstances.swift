/// The whole grid as instance lists, rebuilt row by row from the dirty set.
///
/// Owned by the render thread. A resize, a palette switch or an atlas reset changes every
/// row's instances, so those rebuild everything; a delta rebuilds only its rows.
struct GridInstances {
    private(set) var rows: [RowInstances] = []
    private var cols = 0
    private var palette: CellPalette?
    private var metrics: GridMetrics?
    private var atlasGeneration = -1
    /// `CellPalette.readable` is a binary search over NSColor; a screen uses a handful of
    /// colours, so each is solved once per palette.
    private var readable: [UInt32: UInt32] = [:]

    /// Rows rebuilt by the last `update`, for tests and the debug log.
    private(set) var lastRebuilt = 0

    mutating func update(
        from frame: GridFrame,
        atlasGeneration generation: Int,
        shape: RowInstances.Shaper? = nil,
        glyph: (GlyphKey) -> AtlasSlot?
    ) {
        let reshaped = frame.rows != rows.count || frame.cols != cols
            || frame.palette != palette || frame.metrics != metrics
            || generation != atlasGeneration
        if reshaped {
            rows = Array(repeating: RowInstances(), count: frame.rows)
            cols = frame.cols
            palette = frame.palette
            metrics = frame.metrics
            atlasGeneration = generation
            readable = [:]
        }
        lastRebuilt = 0
        for row in 0..<frame.rows where reshaped || frame.dirty.contains(row) {
            rows[row] = RowInstances.build(row: row, of: frame, glyph: glyph, shape: shape) { fg in
                if let known = readable[fg] { return known }
                let solved = frame.palette.readable(fg)
                readable[fg] = solved
                return solved
            }
            lastRebuilt += 1
        }
        if lastRebuilt > 0 { flatten() }
    }

    /// Forgets everything so the next update rebuilds every row (hidden pane, new atlas).
    mutating func invalidate() {
        rows = []
        cols = 0
        readable = [:]
        flatten()
    }

    /// The rows' instances concatenated in draw order. Re-flattened only after an update that
    /// rebuilt a row, into the same storage; a frame that only moves the cursor, blinks it or
    /// changes the selection reuses them (and the GPU buffers holding them) untouched.
    private(set) var backgrounds: [RectInstance] = []
    private(set) var glyphs: [GlyphInstance] = []
    private(set) var decorations: [RectInstance] = []
    /// Bumped whenever the flattened lists change, so the renderer knows which of its
    /// rotating GPU buffers already hold them.
    private(set) var version = 0

    private mutating func flatten() {
        backgrounds.removeAll(keepingCapacity: true)
        glyphs.removeAll(keepingCapacity: true)
        decorations.removeAll(keepingCapacity: true)
        for row in rows {
            backgrounds.append(contentsOf: row.backgrounds)
            glyphs.append(contentsOf: row.glyphs)
            decorations.append(contentsOf: row.decorations)
        }
        version += 1
    }

    /// The cursor in its shape: a translucent block, an underline or a bar, and an outline
    /// of the cell when the pane does not have the keyboard.
    static func cursorRects(for frame: GridFrame) -> [RectInstance] {
        let cursor = frame.cursor
        let metrics = frame.metrics
        let index = cursor.y * frame.cols + cursor.x
        // On the left half of a wide character the cursor covers both columns.
        let wide = index < frame.cells.count && frame.cells[index].width == 2 && cursor.x + 1 < frame.cols
        let left = metrics.columnX(cursor.x)
        let top = metrics.rowY(cursor.y)
        let width = metrics.columnX(cursor.x + (wide ? 2 : 1)) - left
        let height = metrics.rowY(cursor.y + 1) - top
        let stroke = max(metrics.lineThickness.rounded(), 1) * 2
        let color = CellPalette.components(frame.palette.foreground, alpha: 0.85)
        if cursor.hollow {
            let thin = max(stroke / 2, 1)
            return [
                RectInstance(origin: SIMD2(left, top), size: SIMD2(width, thin), color: color),
                RectInstance(origin: SIMD2(left, top + height - thin), size: SIMD2(width, thin), color: color),
                RectInstance(origin: SIMD2(left, top), size: SIMD2(thin, height), color: color),
                RectInstance(origin: SIMD2(left + width - thin, top), size: SIMD2(thin, height), color: color),
            ]
        }
        switch cursor.shape {
        case .block:
            // Solid; the character under it is redrawn on top (`cursorGlyph`).
            return [RectInstance(
                origin: SIMD2(left, top), size: SIMD2(width, height),
                color: CellPalette.components(frame.palette.foreground))]
        case .underline:
            return [RectInstance(origin: SIMD2(left, top + height - stroke), size: SIMD2(width, stroke), color: color)]
        case .bar:
            return [RectInstance(origin: SIMD2(left, top), size: SIMD2(stroke, height), color: color)]
        }
    }

    /// The character under a solid block cursor, drawn over the block in the cell's
    /// background colour so it reads as inverted (as Ghostty does). Nil for other shapes,
    /// the hollow outline, a blank cell, or a cell drawn as box-drawing rectangles.
    static func cursorGlyph(for frame: GridFrame, glyph: (GlyphKey) -> AtlasSlot?) -> GlyphInstance? {
        let cursor = frame.cursor
        guard cursor.visible, !cursor.hollow, cursor.shape == .block,
              cursor.x >= 0, cursor.y >= 0, cursor.x < frame.cols, cursor.y < frame.rows
        else { return nil }
        let index = cursor.y * frame.cols + cursor.x
        guard index < frame.cells.count else { return nil }
        let cell = frame.cells[index]
        guard cell.width != 0, cell.codepoint > 32, cell.codepoint != 0x7f,
              !BoxDrawing.handles(cell.codepoint)
        else { return nil }
        let metrics = frame.metrics
        let key = GlyphKey(
            codepoint: cell.codepoint,
            bold: cell.attrs & CellAttrs.bold != 0,
            italic: cell.attrs & CellAttrs.italic != 0,
            span: UInt8(min(Int(cell.width), frame.cols - cursor.x)),
            pixelHeight: UInt16(clamping: Int(metrics.cellHeight.rounded(.up))))
        guard let slot = glyph(key) else { return nil }
        let bg = frame.palette.colors(fg: cell.fg, bg: cell.bg, attrs: cell.attrs).bg
        return GlyphInstance(
            origin: SIMD2(
                metrics.columnX(cursor.x) + Float(slot.bearingX),
                metrics.rowY(cursor.y) + Float(slot.bearingY)),
            size: SIMD2(Float(slot.width), Float(slot.height)),
            uv: SIMD2(Float(slot.x), Float(slot.y)),
            colored: slot.colored ? 1 : 0,
            color: CellPalette.components(bg))
    }

    /// A box-drawing character under a solid block cursor, redrawn in the background colour
    /// like `cursorGlyph` does for font glyphs.
    private static func cursorStrokes(for frame: GridFrame) -> [RectInstance] {
        let cursor = frame.cursor
        guard !cursor.hollow, cursor.shape == .block else { return [] }
        let index = cursor.y * frame.cols + cursor.x
        guard index < frame.cells.count else { return [] }
        let cell = frame.cells[index]
        guard cell.width != 0 else { return [] }
        let metrics = frame.metrics
        let span = min(Int(cell.width), frame.cols - cursor.x)
        let left = metrics.columnX(cursor.x)
        let top = metrics.rowY(cursor.y)
        let bg = frame.palette.colors(fg: cell.fg, bg: cell.bg, attrs: cell.attrs).bg
        return BoxDrawing.rects(
            cell.codepoint, origin: SIMD2(left, top),
            size: SIMD2(metrics.columnX(cursor.x + span) - left, metrics.rowY(cursor.y + 1) - top),
            thin: metrics.lineThickness, color: CellPalette.components(bg)) ?? []
    }

    /// The cursor, selection and scroll thumb, which change without touching any row. The
    /// cursor comes apart from the rest: the glyph under a block cursor is drawn between them.
    static func overlays(for frame: GridFrame) -> (cursor: [RectInstance], rest: [RectInstance]) {
        var cursorRects: [RectInstance] = []
        let cursor = frame.cursor
        if cursor.visible, cursor.x >= 0, cursor.y >= 0, cursor.x < frame.cols, cursor.y < frame.rows {
            cursorRects = Self.cursorRects(for: frame)
            cursorRects.append(contentsOf: cursorStrokes(for: frame))
        }
        var out: [RectInstance] = []
        let metrics = frame.metrics
        for span in frame.selection where span.row >= 0 && span.row < frame.rows {
            let first = max(span.first, 0)
            let last = min(span.last, frame.cols - 1)
            guard first <= last else { continue }
            let left = metrics.columnX(first)
            let top = metrics.rowY(span.row)
            out.append(RectInstance(
                origin: SIMD2(left, top),
                size: SIMD2(metrics.columnX(last + 1) - left, metrics.rowY(span.row + 1) - top),
                color: frame.selectionColor))
        }
        if let thumb = frame.thumb {
            out.append(RectInstance(origin: thumb.origin, size: thumb.size, color: frame.thumbColor))
        }
        return (cursorRects, out)
    }
}
