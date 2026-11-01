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
            rows[row] = RowInstances.build(row: row, of: frame, glyph: glyph) { fg in
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
            return [RectInstance(
                origin: SIMD2(left, top), size: SIMD2(width, height),
                color: CellPalette.components(frame.palette.foreground, alpha: 0.35))]
        case .underline:
            return [RectInstance(origin: SIMD2(left, top + height - stroke), size: SIMD2(width, stroke), color: color)]
        case .bar:
            return [RectInstance(origin: SIMD2(left, top), size: SIMD2(stroke, height), color: color)]
        }
    }

    /// The cursor block and scroll thumb, which change without touching any row.
    static func overlays(for frame: GridFrame) -> [RectInstance] {
        var out: [RectInstance] = []
        let cursor = frame.cursor
        if cursor.visible, cursor.x >= 0, cursor.y >= 0, cursor.x < frame.cols, cursor.y < frame.rows {
            out.append(contentsOf: cursorRects(for: frame))
        }
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
        return out
    }
}
