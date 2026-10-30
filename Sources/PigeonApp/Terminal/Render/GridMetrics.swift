/// Cell geometry in drawable pixels for one backing scale.
struct GridMetrics: Equatable, Sendable {
    var cellWidth: Float
    var cellHeight: Float
    /// Baseline, measured down from the cell's top edge.
    var baseline: Float
    /// Underline/strikethrough stroke.
    var lineThickness: Float
    /// Strikethrough top, measured down from the cell's top edge.
    var strikeOffset: Float
    var scale: Float

    /// Columns and rows start on whole pixels: nearest sampling of an atlas glyph is only
    /// crisp when its quad sits on the pixel grid.
    func columnX(_ column: Int) -> Float { (Float(column) * cellWidth).rounded(.down) }
    func rowY(_ row: Int) -> Float { (Float(row) * cellHeight).rounded(.down) }

    /// The pixel size every rasterized glyph of `span` columns has.
    func glyphSize(span: Int) -> (width: Int, height: Int) {
        (Int((Float(span) * cellWidth).rounded(.up)), Int(cellHeight.rounded(.up)))
    }
}
