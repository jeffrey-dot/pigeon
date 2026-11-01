/// Which rasterized glyph a cell needs. Bold and italic are separate faces, and a wide
/// character is rasterized across both of its columns.
struct GlyphKey: Hashable {
    var codepoint: UInt32
    var bold: Bool
    var italic: Bool
    /// Columns the glyph spans (1 or 2).
    var span: UInt8
    /// Cell height in device pixels: a pane on a 1x display needs its own rasterization.
    var pixelHeight: UInt16
    /// A glyph ID in the primary face chosen by shaping (a ligature or contextual alternate),
    /// drawn instead of `codepoint`'s own glyph; 0 for "the codepoint's glyph".
    var glyph: UInt16 = 0

    /// In a shaped run: this cell is covered by a ligature that starts to its left.
    static let noGlyph: UInt16 = 0xFFFF
}
