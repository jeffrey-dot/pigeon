/// Shelf packing for the glyph texture. Pure bookkeeping: rasterizing and uploading belong
/// to the backend, so the packing rules are testable without a GPU.
///
/// Terminal glyphs are all one cell tall, so shelves waste almost nothing. When the texture
/// is full the whole atlas is dropped and rebuilt from what is on screen (`generation` bumps),
/// which is cheaper to get right than per-glyph eviction and only happens on huge glyph sets.
struct GlyphAtlas {
    let width: Int
    let height: Int
    /// Texels kept free around each glyph so linear sampling never bleeds a neighbour in.
    static let padding = 1

    private(set) var slots: [GlyphKey: AtlasSlot] = [:]
    private(set) var generation = 0
    private var shelfX = 0
    private var shelfY = 0
    private var shelfHeight = 0

    init(width: Int, height: Int) {
        self.width = width
        self.height = height
    }

    func slot(for key: GlyphKey) -> AtlasSlot? { slots[key] }

    /// Reserves space for a `width`×`height` glyph, or `nil` when the texture is full.
    mutating func allocate(
        _ key: GlyphKey, width glyphWidth: Int, height glyphHeight: Int, colored: Bool
    ) -> AtlasSlot? {
        let paddedWidth = glyphWidth + Self.padding
        let paddedHeight = glyphHeight + Self.padding
        guard paddedWidth <= width, paddedHeight <= height else { return nil }
        if shelfX + paddedWidth > width {
            shelfY += shelfHeight
            shelfX = 0
            shelfHeight = 0
        }
        guard shelfY + paddedHeight <= height else { return nil }
        let slot = AtlasSlot(
            x: shelfX, y: shelfY, width: glyphWidth, height: glyphHeight, colored: colored)
        shelfX += paddedWidth
        shelfHeight = max(shelfHeight, paddedHeight)
        slots[key] = slot
        return slot
    }

    /// Forgets every glyph; slots handed out before this are no longer valid.
    mutating func reset() {
        slots = [:]
        shelfX = 0
        shelfY = 0
        shelfHeight = 0
        generation += 1
    }
}
