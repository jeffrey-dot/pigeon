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
    }

    /// Forgets everything so the next update rebuilds every row (hidden pane, new atlas).
    mutating func invalidate() {
        rows = []
        cols = 0
        readable = [:]
    }

    /// Background, decoration and overlay rectangles in draw order.
    func rects(overlays: [RectInstance]) -> (backgrounds: [RectInstance], decorations: [RectInstance]) {
        var backgrounds: [RectInstance] = []
        var decorations: [RectInstance] = []
        for row in rows {
            backgrounds.append(contentsOf: row.backgrounds)
            decorations.append(contentsOf: row.decorations)
        }
        decorations.append(contentsOf: overlays)
        return (backgrounds, decorations)
    }

    func glyphs() -> [GlyphInstance] {
        rows.flatMap(\.glyphs)
    }

    /// The cursor block and scroll thumb, which change without touching any row.
    static func overlays(for frame: GridFrame) -> [RectInstance] {
        var out: [RectInstance] = []
        let cursor = frame.cursor
        if cursor.visible, cursor.x >= 0, cursor.y >= 0, cursor.x < frame.cols, cursor.y < frame.rows {
            let metrics = frame.metrics
            let left = metrics.columnX(cursor.x)
            let top = metrics.rowY(cursor.y)
            out.append(RectInstance(
                origin: SIMD2(left, top),
                size: SIMD2(metrics.columnX(cursor.x + 1) - left, metrics.rowY(cursor.y + 1) - top),
                color: CellPalette.components(frame.palette.foreground, alpha: 0.35)))
        }
        if let thumb = frame.thumb {
            out.append(RectInstance(origin: thumb.origin, size: thumb.size, color: frame.thumbColor))
        }
        return out
    }
}
