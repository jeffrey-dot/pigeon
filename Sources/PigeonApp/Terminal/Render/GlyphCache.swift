import Metal

/// The two atlas textures and their packing state: an 8-bit mask atlas for ordinary glyphs
/// and a BGRA atlas for colour glyphs (emoji), so text never pays four bytes per texel.
///
/// Render-thread only. A full atlas is reset and every glyph re-rasterized on demand;
/// `generation` tells `GridInstances` that the slots it holds are stale.
final class GlyphCache {
    static let side = 2048

    let mask: MTLTexture
    let color: MTLTexture
    private var maskAtlas = GlyphAtlas(width: GlyphCache.side, height: GlyphCache.side)
    private var colorAtlas = GlyphAtlas(width: GlyphCache.side, height: GlyphCache.side)
    /// Glyphs that rasterize to nothing (a font without the character and no fallback):
    /// remembered so a screen full of them is not re-rasterized every frame.
    private var blank: Set<GlyphKey> = []
    private(set) var generation = 0
    /// Set when a lookup filled an atlas mid-frame; the frame is rebuilt after the reset.
    private(set) var overflowed = false

    init?(device: MTLDevice) {
        let maskDescriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .r8Unorm, width: Self.side, height: Self.side, mipmapped: false)
        maskDescriptor.usage = .shaderRead
        let colorDescriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm, width: Self.side, height: Self.side, mipmapped: false)
        colorDescriptor.usage = .shaderRead
        guard let mask = device.makeTexture(descriptor: maskDescriptor),
              let color = device.makeTexture(descriptor: colorDescriptor)
        else { return nil }
        self.mask = mask
        self.color = color
    }

    /// The slot for `key`, rasterizing and uploading it on first use.
    func slot(for key: GlyphKey, metrics: GridMetrics) -> AtlasSlot? {
        if let slot = maskAtlas.slot(for: key) ?? colorAtlas.slot(for: key) { return slot }
        guard !blank.contains(key), !overflowed else { return nil }
        guard let bitmap = CoreTextRenderer.rasterize(key, metrics: metrics) else {
            blank.insert(key)
            return nil
        }
        let allocated = bitmap.colored
            ? colorAtlas.allocate(key, width: bitmap.width, height: bitmap.height, colored: true)
            : maskAtlas.allocate(key, width: bitmap.width, height: bitmap.height, colored: false)
        guard let slot = allocated else {
            overflowed = true
            return nil
        }
        let texture = bitmap.colored ? color : mask
        bitmap.bytes.withUnsafeBytes { buffer in
            guard let base = buffer.baseAddress else { return }
            texture.replace(
                region: MTLRegionMake2D(slot.x, slot.y, slot.width, slot.height),
                mipmapLevel: 0,
                withBytes: base,
                bytesPerRow: bitmap.colored ? bitmap.width * 4 : bitmap.width)
        }
        return slot
    }

    /// Called after a frame that overflowed: start both atlases over.
    func resetIfOverflowed() -> Bool {
        guard overflowed else { return false }
        maskAtlas.reset()
        colorAtlas.reset()
        blank = []
        overflowed = false
        generation += 1
        return true
    }
}
