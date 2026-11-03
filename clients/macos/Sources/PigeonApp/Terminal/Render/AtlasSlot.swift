/// A glyph's place in the atlas texture, in texels.
struct AtlasSlot: Equatable {
    var x: Int
    var y: Int
    var width: Int
    var height: Int
    /// Colour glyphs (emoji) keep their own pixels; mask glyphs are tinted with the cell's fg.
    var colored: Bool
    /// The bitmap's top-left corner relative to the cell's, in pixels: the bitmap covers the
    /// glyph's ink, which may start left of or above the cell (italic overhang, accents).
    var bearingX = 0
    var bearingY = 0
}
