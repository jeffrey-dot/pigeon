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
}
