/// A glyph's place in the atlas texture, in texels.
struct AtlasSlot: Equatable {
    var x: Int
    var y: Int
    var width: Int
    var height: Int
    /// Colour glyphs (emoji) keep their own pixels; mask glyphs are tinted with the cell's fg.
    var colored: Bool
}
