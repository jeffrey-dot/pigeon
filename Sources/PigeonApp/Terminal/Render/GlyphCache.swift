import Foundation
import Metal

/// The two atlas textures and their packing state: an 8-bit mask atlas for ordinary glyphs
/// and a BGRA atlas for colour glyphs (emoji), so text never pays four bytes per texel.
///
/// One cache per backing scale is shared by every pane's render thread (`shared`), so each
/// glyph is rasterized and stored once, not once per pane; a lock guards all of it. A full
/// atlas doubles (up to `maxSide`) and keeps every slot; only one at its cap is reset, with
/// every glyph re-rasterized on demand. `generation` tells `GridInstances` that the slots it
/// holds are stale.
final class GlyphCache: @unchecked Sendable {
    /// Atlases start small (a screen of Latin text fits in a fraction of 1024²) and double.
    static let initialSide = 1024
    /// 8192² is 64 MB of mask; colour atlases stop at half that side (64 MB of BGRA).
    static let maxSide = 8192

    private let device: MTLDevice
    private let lock = NSLock()
    private var mask: MTLTexture
    private var color: MTLTexture
    private var maskAtlas: GlyphAtlas
    private var colorAtlas: GlyphAtlas
    private let maxMaskSide: Int
    private let maxColorSide: Int
    /// Glyphs that rasterize to nothing (a font without the character and no fallback):
    /// remembered so a screen full of them is not re-rasterized every frame.
    private var blank: Set<GlyphKey> = []
    private var currentGeneration = 0
    /// Set when a lookup found an atlas full at its cap; the frame is rebuilt after the reset.
    private var overflowed = false

    var generation: Int {
        lock.lock()
        defer { lock.unlock() }
        return currentGeneration
    }

    init?(device: MTLDevice, side: Int = GlyphCache.initialSide, maxSide: Int = GlyphCache.maxSide) {
        self.device = device
        maxMaskSide = max(maxSide, side)
        maxColorSide = max(maxSide / 2, side)
        guard let mask = Self.texture(device, .r8Unorm, side: side),
              let color = Self.texture(device, .bgra8Unorm, side: side)
        else { return nil }
        self.mask = mask
        self.color = color
        maskAtlas = GlyphAtlas(width: side, height: side)
        colorAtlas = GlyphAtlas(width: side, height: side)
    }

    private struct Entry {
        weak var cache: GlyphCache?
    }

    private static let registryLock = NSLock()
    nonisolated(unsafe) private static var registry: [Float: Entry] = [:]

    /// The cache every pane drawing at `scale` uses. Held weakly here: when the last renderer
    /// on that scale goes away (all panes hidden), so do its textures.
    static func shared(device: MTLDevice, scale: Float) -> GlyphCache? {
        registryLock.lock()
        defer { registryLock.unlock() }
        if let cache = registry[scale]?.cache, cache.device === device { return cache }
        guard let cache = GlyphCache(device: device) else { return nil }
        registry[scale] = Entry(cache: cache)
        return cache
    }

    private static func texture(_ device: MTLDevice, _ format: MTLPixelFormat, side: Int) -> MTLTexture? {
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: format, width: side, height: side, mipmapped: false)
        descriptor.usage = .shaderRead
        // CPU-readable, so growing can copy the old texels across with `getBytes`.
        descriptor.storageMode = device.hasUnifiedMemory ? .shared : .managed
        return device.makeTexture(descriptor: descriptor)
    }

    /// The current textures and the generation their slots belong to, read together: a frame
    /// whose instances were built for another generation must rebuild before drawing.
    func textures() -> (mask: MTLTexture, color: MTLTexture, generation: Int) {
        lock.lock()
        defer { lock.unlock() }
        return (mask, color, currentGeneration)
    }

    /// The slot for `key`, rasterizing and uploading it on first use.
    func slot(for key: GlyphKey, metrics: GridMetrics) -> AtlasSlot? {
        lock.lock()
        defer { lock.unlock() }
        if let slot = maskAtlas.slot(for: key) ?? colorAtlas.slot(for: key) { return slot }
        guard !blank.contains(key), !overflowed else { return nil }
        guard let bitmap = CoreTextRenderer.rasterize(key, metrics: metrics) else {
            blank.insert(key)
            return nil
        }
        guard var slot = allocate(key, bitmap) else {
            overflowed = true
            return nil
        }
        slot.bearingX = bitmap.bearingX
        slot.bearingY = bitmap.bearingY
        if bitmap.colored {
            colorAtlas.update(key, slot)
        } else {
            maskAtlas.update(key, slot)
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

    /// Packs `bitmap`, doubling its atlas as often as needed before giving up.
    private func allocate(_ key: GlyphKey, _ bitmap: CoreTextRenderer.GlyphBitmap) -> AtlasSlot? {
        while true {
            let slot = bitmap.colored
                ? colorAtlas.allocate(key, width: bitmap.width, height: bitmap.height, colored: true)
                : maskAtlas.allocate(key, width: bitmap.width, height: bitmap.height, colored: false)
            if let slot { return slot }
            guard grow(colored: bitmap.colored) else { return nil }
        }
    }

    /// Doubles one atlas texture, copying its texels; `false` at the cap or out of memory.
    private func grow(colored: Bool) -> Bool {
        let old = colored ? color : mask
        let side = old.width * 2
        guard side <= (colored ? maxColorSide : maxMaskSide),
              let grown = Self.texture(device, old.pixelFormat, side: side)
        else { return false }
        let bytesPerTexel = colored ? 4 : 1
        var texels = [UInt8](repeating: 0, count: old.width * old.height * bytesPerTexel)
        let region = MTLRegionMake2D(0, 0, old.width, old.height)
        texels.withUnsafeMutableBytes { buffer in
            guard let base = buffer.baseAddress else { return }
            old.getBytes(base, bytesPerRow: old.width * bytesPerTexel, from: region, mipmapLevel: 0)
            grown.replace(region: region, mipmapLevel: 0, withBytes: base,
                          bytesPerRow: old.width * bytesPerTexel)
        }
        // Frames already encoded keep the old texture alive until the GPU is done with it.
        if colored {
            color = grown
            colorAtlas.grow(width: side, height: side)
        } else {
            mask = grown
            maskAtlas.grow(width: side, height: side)
        }
        return true
    }

    /// Called after a frame that overflowed: start both atlases over.
    func resetIfOverflowed() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard overflowed else { return false }
        // Fresh textures rather than overwriting the old ones: another pane's frame may still
        // be sampling them on the GPU.
        if let freshMask = Self.texture(device, .r8Unorm, side: mask.width),
           let freshColor = Self.texture(device, .bgra8Unorm, side: color.width) {
            mask = freshMask
            color = freshColor
        }
        maskAtlas.reset()
        colorAtlas.reset()
        blank = []
        overflowed = false
        currentGeneration += 1
        return true
    }
}
