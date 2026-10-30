/// Everything one frame of a pane is drawn from, captured on the main thread and handed to
/// the render thread as a value. The cell array is copy-on-write, so the hand-off itself is a
/// reference bump; the main thread pays for a copy only when it patches the next delta.
struct GridFrame: Sendable {
    var cells: [PaneView.Cell]
    var cols: Int
    var rows: Int
    var cursor: PaneView.Cursor
    var palette: CellPalette
    var metrics: GridMetrics
    /// Drawable size in pixels.
    var width: Int
    var height: Int
    /// Scroll indicator in drawable pixels (top-left origin), or nil when there is no history.
    var thumb: (origin: SIMD2<Float>, size: SIMD2<Float>)?
    var thumbColor: SIMD4<Float>
    /// Rows changed since the previous frame handed over.
    var dirty: DirtyRows

    /// A frame the render thread has not drawn yet is replaced by a newer one; the rows the
    /// older one changed must still be rebuilt.
    func superseding(_ older: GridFrame) -> GridFrame {
        var merged = self
        merged.dirty = dirty.union(older.dirty)
        return merged
    }
}
