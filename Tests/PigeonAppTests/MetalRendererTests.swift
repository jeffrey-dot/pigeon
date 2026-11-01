import AppKit
import Metal
import XCTest
@testable import PigeonApp

/// The GPU renderer's CPU-side rules: atlas packing, dirty-row bookkeeping, instance
/// building and the main→render hand-off. None of these need a GPU.
final class MetalRendererTests: XCTestCase {
    private let metrics = GridMetrics(
        cellWidth: 16, cellHeight: 31.2, baseline: 24, lineThickness: 2, strikeOffset: 16, scale: 2)
    private let palette = CellPalette(
        ansi: Array(repeating: 0x123456, count: 16), foreground: 0x111111, background: 0xffffff)

    private func key(_ codepoint: UInt32) -> GlyphKey {
        GlyphKey(codepoint: codepoint, bold: false, italic: false, span: 1, pixelHeight: 32)
    }

    private func frame(
        _ text: String, cols: Int, attrs: UInt8 = 0, bg: UInt32 = 0, dirty: DirtyRows = .everything
    ) -> GridFrame {
        let cells = text.unicodeScalars.map {
            PaneView.Cell(codepoint: $0.value, attrs: attrs, fg: 0, bg: bg, width: 1)
        }
        return GridFrame(
            cells: cells, cols: cols, rows: cells.count / cols,
            cursor: PaneView.Cursor(x: 0, y: 0, visible: false), palette: palette,
            metrics: metrics, width: 100, height: 100, thumb: nil,
            thumbColor: SIMD4(0, 0, 0, 0), dirty: dirty)
    }

    private func slot(_ key: GlyphKey) -> AtlasSlot? {
        AtlasSlot(x: Int(key.codepoint), y: 0, width: 16, height: 32, colored: false)
    }

    // MARK: - atlas

    func testTheAtlasPacksShelvesLeftToRightThenDown() {
        var atlas = GlyphAtlas(width: 40, height: 40)
        let a = atlas.allocate(key(65), width: 16, height: 10, colored: false)
        let b = atlas.allocate(key(66), width: 16, height: 10, colored: false)
        let c = atlas.allocate(key(67), width: 16, height: 10, colored: false)
        XCTAssertEqual(a?.x, 0)
        XCTAssertEqual(b?.x, 17, "one texel of padding between neighbours")
        XCTAssertEqual(c?.x, 0, "the third glyph does not fit and starts the next shelf")
        XCTAssertEqual(c?.y, 11)
        XCTAssertEqual(atlas.slot(for: key(66)), b)
    }

    func testAFullAtlasRefusesAndAResetStartsOver() {
        var atlas = GlyphAtlas(width: 20, height: 20)
        XCTAssertNotNil(atlas.allocate(key(65), width: 18, height: 18, colored: false))
        XCTAssertNil(atlas.allocate(key(66), width: 18, height: 18, colored: false))
        XCTAssertNil(atlas.allocate(key(67), width: 30, height: 4, colored: false), "wider than the texture")
        atlas.reset()
        XCTAssertEqual(atlas.generation, 1)
        XCTAssertNil(atlas.slot(for: key(65)), "slots from before the reset are gone")
        XCTAssertEqual(atlas.allocate(key(66), width: 18, height: 18, colored: false)?.x, 0)
    }

    func testAGrownAtlasKeepsItsSlotsAndTakesMore() {
        var atlas = GlyphAtlas(width: 20, height: 20)
        let first = atlas.allocate(key(65), width: 18, height: 18, colored: false)
        XCTAssertNil(atlas.allocate(key(66), width: 18, height: 18, colored: false))
        atlas.grow(width: 40, height: 40)
        XCTAssertEqual(atlas.slot(for: key(65)), first, "growing moves nothing")
        XCTAssertEqual(atlas.generation, 0)
        XCTAssertNotNil(atlas.allocate(key(66), width: 18, height: 18, colored: false))
    }

    /// A full atlas used to be reset, re-rasterizing the whole screen; it now doubles first,
    /// and panes on the same display share one cache.
    func testTheGlyphCacheGrowsBeforeResettingAndIsShared() throws {
        guard let device = MTLCreateSystemDefaultDevice() else { throw XCTSkip("no Metal device") }
        let metrics = CoreTextRenderer.gridMetrics(scale: 2)
        let cache = try XCTUnwrap(GlyphCache(device: device, side: 64, maxSide: 256))
        let pixelHeight = UInt16(metrics.cellHeight)
        var slots: [GlyphKey: AtlasSlot] = [:]
        for codepoint in UInt32(0x41)...0x5A {
            let glyph = GlyphKey(codepoint: codepoint, bold: false, italic: false, span: 1, pixelHeight: pixelHeight)
            slots[glyph] = cache.slot(for: glyph, metrics: metrics)
        }
        XCTAssertEqual(slots.values.count, 26)
        XCTAssertGreaterThan(cache.textures().mask.width, 64, "doubled instead of reset")
        XCTAssertEqual(cache.generation, 0)
        XCTAssertFalse(cache.resetIfOverflowed())
        for (glyph, slot) in slots {
            XCTAssertEqual(cache.slot(for: glyph, metrics: metrics), slot, "slots survive growth")
        }
        // Past the cap, a reset is the last resort.
        let before = cache.textures().mask
        for codepoint in UInt32(0x4E00)...0x4F00 {
            _ = cache.slot(for: GlyphKey(
                codepoint: codepoint, bold: false, italic: false, span: 2, pixelHeight: pixelHeight),
                metrics: metrics)
        }
        XCTAssertTrue(cache.resetIfOverflowed())
        XCTAssertFalse(cache.textures().mask === before, "a reset never rewrites a texture in use")
        XCTAssertEqual(cache.generation, 1)

        let a = GlyphCache.shared(device: device, scale: 2)
        let b = GlyphCache.shared(device: device, scale: 2)
        XCTAssertTrue(a != nil && a === b, "one cache per display scale")
        XCTAssertFalse(a === GlyphCache.shared(device: device, scale: 1))
    }

    // MARK: - dirty rows

    func testDirtyRowsMergeWithoutLosingAnything() {
        var first = DirtyRows.none
        first.mark(1)
        var second = DirtyRows.none
        second.mark(3)
        let merged = first.union(second)
        XCTAssertTrue(merged.contains(1))
        XCTAssertTrue(merged.contains(3))
        XCTAssertFalse(merged.contains(2))
        XCTAssertTrue(merged.union(.everything).contains(2), "a full invalidation wins")
        XCTAssertTrue(DirtyRows.none.isEmpty)
    }

    func testANewerFrameKeepsTheOlderFramesDirtyRows() {
        let mailbox = FrameMailbox()
        var older = DirtyRows.none
        older.mark(0)
        var newer = DirtyRows.none
        newer.mark(1)
        mailbox.post(frame("ab", cols: 1, dirty: older))
        mailbox.post(frame("cd", cols: 1, dirty: newer))
        let taken = mailbox.poll()
        XCTAssertEqual(taken?.cells.first?.codepoint, 99, "the newest screen is the one drawn")
        XCTAssertEqual(taken?.dirty.contains(0), true)
        XCTAssertEqual(taken?.dirty.contains(1), true)
        XCTAssertNil(mailbox.poll())
    }

    /// Only a post into an empty mailbox wakes the render thread; later ones ride along.
    func testOnlyThePostIntoAnEmptyMailboxAsksForAWakeup() {
        let mailbox = FrameMailbox()
        XCTAssertTrue(mailbox.post(frame("ab", cols: 1)))
        XCTAssertFalse(mailbox.post(frame("cd", cols: 1)))
        _ = mailbox.poll()
        XCTAssertTrue(mailbox.post(frame("ef", cols: 1)))
    }

    /// Every delta used to post a frame at once, so the mailbox shared `cells` and the next
    /// delta copied the whole grid. Changes now coalesce into one post per run-loop turn.
    @MainActor
    func testScreenChangesCoalesceIntoOnePostPerRunLoopTurn() {
        let view = PaneView(frame: .zero)
        view.screenChanged()
        view.screenChanged()
        XCTAssertTrue(view.postScheduled)
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        XCTAssertFalse(view.postScheduled, "the scheduled post ran once and cleared the flag")
    }

    func testARestoredFrameNeverOverwritesANewerOne() {
        let mailbox = FrameMailbox()
        var taken = DirtyRows.none
        taken.mark(0)
        mailbox.post(frame("cd", cols: 1, dirty: .none))
        mailbox.restore(frame("ab", cols: 1, dirty: taken))
        let next = mailbox.poll()
        XCTAssertEqual(next?.cells.first?.codepoint, 99, "the newer screen stays")
        XCTAssertEqual(next?.dirty.contains(0), true, "the restored frame's rows are still redrawn")
        mailbox.restore(frame("ab", cols: 1))
        XCTAssertEqual(mailbox.poll()?.cells.first?.codepoint, 97, "an empty mailbox takes it back")
    }

    func testAClosedMailboxHandsOutNothing() {
        let mailbox = FrameMailbox()
        mailbox.post(frame("ab", cols: 1))
        mailbox.close()
        XCTAssertTrue(mailbox.isClosed)
        XCTAssertNil(mailbox.poll(), "a stopped renderer must not draw a frame posted before stop")
        mailbox.post(frame("cd", cols: 1))
        XCTAssertNil(mailbox.poll())
    }

    // MARK: - instances

    func testOnlyDirtyRowsAreRebuilt() {
        var grid = GridInstances()
        grid.update(from: frame("abcd", cols: 2), atlasGeneration: 0, glyph: slot)
        XCTAssertEqual(grid.lastRebuilt, 2)
        var dirty = DirtyRows.none
        dirty.mark(1)
        grid.update(from: frame("abxy", cols: 2, dirty: dirty), atlasGeneration: 0, glyph: slot)
        XCTAssertEqual(grid.lastRebuilt, 1)
        XCTAssertEqual(grid.rows[1].glyphs.map(\.uv.x), [120, 121])
        grid.update(from: frame("abxy", cols: 2, dirty: .none), atlasGeneration: 1, glyph: slot)
        XCTAssertEqual(grid.lastRebuilt, 2, "an atlas reset invalidates every row's slots")
    }

    /// The flattened instance lists used to be rebuilt into fresh arrays every frame; a frame
    /// that only moves or blinks the cursor must now leave them (and their version) alone.
    func testACursorOnlyFrameReusesTheFlattenedInstances() {
        var grid = GridInstances()
        grid.update(from: frame("abcd", cols: 2), atlasGeneration: 0, glyph: slot)
        XCTAssertEqual(grid.glyphs.count, 4)
        let version = grid.version
        var blink = frame("abcd", cols: 2, dirty: .none)
        blink.cursor = PaneView.Cursor(x: 1, y: 1, visible: true)
        grid.update(from: blink, atlasGeneration: 0, glyph: slot)
        XCTAssertEqual(grid.lastRebuilt, 0)
        XCTAssertEqual(grid.version, version, "nothing to re-flatten or re-upload")
        var dirty = DirtyRows.none
        dirty.mark(0)
        grid.update(from: frame("xycd", cols: 2, dirty: dirty), atlasGeneration: 0, glyph: slot)
        XCTAssertEqual(grid.version, version + 1)
        XCTAssertEqual(grid.glyphs.map(\.uv.x), [120, 121, 99, 100])
    }

    func testBlanksDrawNothingAndThemeBackgroundsAreLeftToTheClearColour() {
        let row = RowInstances.build(row: 0, of: frame("a b", cols: 3), glyph: slot) { $0 }
        XCTAssertEqual(row.glyphs.count, 2)
        XCTAssertTrue(row.backgrounds.isEmpty)
        let coloured = RowInstances.build(
            row: 0, of: frame("a", cols: 1, bg: 0x00ff0000), glyph: slot) { $0 }
        XCTAssertEqual(coloured.backgrounds.first?.color, SIMD4(1, 0, 0, 1))
    }

    func testAWideGlyphSpansTwoColumnsAndItsContinuationIsSkipped() {
        var wide = frame("中 x", cols: 3)
        wide.cells[0].width = 2
        wide.cells[1].width = 0
        var spans: [UInt8] = []
        let row = RowInstances.build(row: 0, of: wide, glyph: { key in
            spans.append(key.span)
            return self.slot(key)
        }) { $0 }
        XCTAssertEqual(spans, [2, 1])
        XCTAssertEqual(row.glyphs.map(\.origin.x), [0, 32])
    }

    func testUnderlineAndStrikethroughBecomeStrokesInTheTextColour() {
        let attrs = CellAttrs.underline | CellAttrs.strikethrough
        let row = RowInstances.build(row: 0, of: frame("a", cols: 1, attrs: attrs), glyph: slot) { _ in 0x00ff00 }
        XCTAssertEqual(row.decorations.count, 2)
        XCTAssertEqual(row.decorations.first?.color, SIMD4(0, 1, 0, 1))
        XCTAssertEqual(row.decorations.first?.size.y, 2)
    }

    func testReverseVideoSwapsTheResolvedColours() {
        let colors = palette.colors(fg: 0, bg: 0, attrs: CellAttrs.reverse)
        XCTAssertEqual(colors.fg, 0xffffff)
        XCTAssertEqual(colors.bg, 0x111111)
    }

    func testPaletteIndicesResolveThroughTheThemeAndTheXtermCube() {
        let resolved = CellPalette.resolve(for: NSAppearance(named: .darkAqua)!)
        XCTAssertEqual(resolved.rgb(0x0100_0001), TerminalPalette.hex(1, appearance: NSAppearance(named: .darkAqua)!))
        XCTAssertEqual(resolved.rgb(0x0100_0000 | 196), 0xff0000, "xterm cube")
        XCTAssertEqual(resolved.rgb(0x0100_0000 | 232), 0x080808, "grey ramp")
        XCTAssertEqual(resolved.rgb(0x00ab_cdef), 0xabcdef)
    }

    // MARK: - frame stats

    func testPercentilesUseNearestRank() {
        let samples = (1...100).map(Double.init)
        XCTAssertEqual(FrameStats.percentile(samples, 50), 50)
        XCTAssertEqual(FrameStats.percentile(samples, 99), 99)
        XCTAssertEqual(FrameStats.percentile([7], 99), 7)
        XCTAssertNil(FrameStats.percentile([], 50))
    }

    func testTheSummaryReportsOncePerSecondAndStartsAFreshWindow() {
        var stats = FrameStats(renderer: "metal")
        XCTAssertNil(stats.summary(), "no frames, no line")
        stats.reportIfDue(now: 1_000)
        stats.record(cpuMicros: 400)
        stats.record(cpuMicros: 2_000)
        stats.record(gpuMicros: 120)
        XCTAssertEqual(
            stats.summary(),
            "pigeon: frames renderer=metal n=2 cpu_p50=400us cpu_p99=2000us gpu_p50=120us gpu_p99=120us")
        stats.reportIfDue(now: 500_000_000)
        XCTAssertEqual(stats.cpu.count, 2, "half a second in: keep collecting")
        stats.reportIfDue(now: 1_000_001_000)
        XCTAssertTrue(stats.cpu.isEmpty)
        XCTAssertTrue(stats.gpu.isEmpty)
    }

    // MARK: - glyph bounds

    /// Whether any texel on the bitmap's outer border has ink: a glyph cut off by its bitmap.
    private func touchesEdge(_ bitmap: CoreTextRenderer.GlyphBitmap) -> Bool {
        let stride = bitmap.colored ? 4 : 1
        func ink(_ x: Int, _ y: Int) -> Bool {
            let base = (y * bitmap.width + x) * stride
            return bitmap.colored ? bitmap.bytes[base + 3] > 0 : bitmap.bytes[base] > 0
        }
        for x in 0..<bitmap.width where ink(x, 0) || ink(x, bitmap.height - 1) { return true }
        for y in 0..<bitmap.height where ink(0, y) || ink(bitmap.width - 1, y) { return true }
        return false
    }

    private func edgeReport(_ bitmap: CoreTextRenderer.GlyphBitmap) -> String {
        let s = bitmap.colored ? 4 : 1
        func a(_ x: Int, _ y: Int) -> UInt8 { bitmap.bytes[(y * bitmap.width + x) * s + (s - 1)] }
        let top = (0..<bitmap.width).map { a($0, 0) }.max() ?? 0
        let bottom = (0..<bitmap.width).map { a($0, bitmap.height - 1) }.max() ?? 0
        let l = (0..<bitmap.height).map { a(0, $0) }.max() ?? 0
        let r = (0..<bitmap.height).map { a(bitmap.width - 1, $0) }.max() ?? 0
        return "t\(top) b\(bottom) l\(l) r\(r)"
    }

    private func rasterized(_ codepoint: UInt32, italic: Bool = false, span: UInt8 = 1) throws
        -> CoreTextRenderer.GlyphBitmap {
        let metrics = CoreTextRenderer.gridMetrics(scale: 2)
        return try XCTUnwrap(CoreTextRenderer.rasterize(
            GlyphKey(codepoint: codepoint, bold: false, italic: italic, span: span,
                     pixelHeight: UInt16(metrics.cellHeight)),
            metrics: metrics))
    }

    /// Italic overhang, descenders and wide fallback glyphs used to be cropped to the cell.
    func testGlyphBitmapsHoldTheWholeInk() throws {
        for (codepoint, italic) in [(0x57, true), (0x66, true), (0x67, false), (0xC5, false)] {
            let bitmap = try rasterized(UInt32(codepoint), italic: italic)
            XCTAssertFalse(touchesEdge(bitmap), "U+\(String(codepoint, radix: 16)) is clipped")
        }
        let emoji = try rasterized(0x1F600, span: 1)
        XCTAssertFalse(touchesEdge(emoji), "an emoji in one column is scaled, not cropped \(emoji.width)x\(emoji.height) \(emoji.bearingX),\(emoji.bearingY) \(edgeReport(emoji))")
    }

    /// A fallback glyph larger than its cells is scaled down to fit them (plus the 1px margin).
    func testOversizedFallbackGlyphsAreScaledIntoTheirCells() throws {
        let metrics = CoreTextRenderer.gridMetrics(scale: 2)
        let emoji = try rasterized(0x1F600, span: 1)
        XCTAssertLessThanOrEqual(Float(emoji.width), metrics.cellWidth + 2)
        XCTAssertLessThanOrEqual(Float(emoji.height), metrics.cellHeight + 2)
        XCTAssertGreaterThanOrEqual(emoji.bearingX, -1)
        XCTAssertGreaterThanOrEqual(emoji.bearingY, -1)
    }

    func testAGlyphIsPlacedByItsBearings() {
        var frame = frame("a", cols: 1)
        frame.cells[0].codepoint = 0x61
        let row = RowInstances.build(row: 0, of: frame, glyph: { _ in
            AtlasSlot(x: 5, y: 6, width: 10, height: 12, colored: false, bearingX: -2, bearingY: 7)
        }) { $0 }
        XCTAssertEqual(row.glyphs.first?.origin, SIMD2(-2, 7))
        XCTAssertEqual(row.glyphs.first?.size, SIMD2(10, 12))
    }

    // MARK: - GPU smoke

    func testTheShadersCompileAndAGlyphRasterizes() throws {
        guard MTLCreateSystemDefaultDevice() != nil else { throw XCTSkip("no Metal device") }
        XCTAssertNotNil(MetalGridRenderer(), "pipelines must build from MetalShaders.source")
        let metrics = CoreTextRenderer.gridMetrics(scale: 2)
        let pixelHeight = UInt16(metrics.cellHeight.rounded(.up))
        let bitmap = CoreTextRenderer.rasterize(
            GlyphKey(codepoint: 0x41, bold: false, italic: false, span: 1, pixelHeight: pixelHeight),
            metrics: metrics)
        XCTAssertEqual(bitmap?.colored, false)
        XCTAssertTrue(bitmap?.bytes.contains { $0 > 128 } ?? false, "'A' leaves ink in the mask")
        let emoji = CoreTextRenderer.rasterize(
            GlyphKey(codepoint: 0x1F600, bold: false, italic: false, span: 2, pixelHeight: pixelHeight),
            metrics: metrics)
        XCTAssertEqual(emoji?.colored, true, "emoji go to the colour atlas")
    }
}
