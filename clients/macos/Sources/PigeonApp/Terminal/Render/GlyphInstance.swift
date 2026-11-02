/// GPU instance layout. Field order and sizes must match the struct in `MetalShaders`:
/// `float2` is 8 bytes and `float4` 16-byte aligned, so the stride lands on a multiple of 16.
/// One glyph quad: where it goes, where its pixels are in the atlas, and how to tint it.
struct GlyphInstance: Equatable {
    var origin: SIMD2<Float>
    var size: SIMD2<Float>
    /// Atlas texel of the glyph's top-left corner; the quad maps 1:1 onto texels.
    var uv: SIMD2<Float>
    /// 1 samples the colour atlas as is (emoji); 0 tints the mask atlas with `color`.
    var colored: Float
    var padding: Float = 0
    var color: SIMD4<Float>
}
