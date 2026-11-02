/// GPU instance layout. Field order and sizes must match the struct in `MetalShaders`:
/// `float2` is 8 bytes and `float4` 16-byte aligned, so the stride lands on a multiple of 16.
/// A filled rectangle in drawable pixels, top-left origin: cell backgrounds, underlines,
/// strikethrough, the cursor and the scroll thumb.
struct RectInstance: Equatable {
    var origin: SIMD2<Float>
    var size: SIMD2<Float>
    /// Straight alpha; the shader premultiplies.
    var color: SIMD4<Float>
}
