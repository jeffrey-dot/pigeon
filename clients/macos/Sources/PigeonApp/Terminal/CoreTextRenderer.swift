import AppKit
import CoreText

/// Font metrics and glyph rasterization for the Metal grid renderer. The only file allowed
/// to import CoreText; the grid itself is drawn on the GPU (`MetalGridRenderer`).
enum CoreTextRenderer {
    /// Read once at launch (`~/.config/pigeon/config.toml`, `FontConfig`): metrics are cached
    /// below, so a change applies on the next launch.
    static let config = FontConfig.load()
    static var fontSize: CGFloat { CGFloat(config.size) }
    static var lineHeightRatio: CGFloat { CGFloat(config.lineHeight) }

    static func font(bold: Bool) -> NSFont { bold ? boldFont : regularFont }

    /// Looked up once: `NSFont(name:)` walks the font registry, and every frame, mouse event
    /// and resize asks for the cell size. `static let` is lazily and thread-safely initialized,
    /// so the render thread's rasterizer can share it.
    private static let regularFont = loadFont(bold: false)
    private static let boldFont = loadFont(bold: true)
    /// The cell in points as the font defines it, before snapping to the pixel grid.
    private static let naturalCellSize: CGSize = {
        let advance = ("M" as NSString).size(withAttributes: [.font: regularFont])
        // Cell height is the font size times the line-height ratio, not the font's own
        // ascender/descender span: that is what gives the rows their airiness.
        return CGSize(width: max(advance.width, 1), height: max(fontSize * lineHeightRatio, 1))
    }()

    /// The configured family when it is installed, otherwise the system monospace (SF Mono).
    /// Bold is derived with CoreText rather than `NSFontManager`: the render thread is the
    /// first to ask for it, and `NSFontManager` is main-thread AppKit.
    private static func loadFont(bold: Bool) -> NSFont {
        guard !config.family.isEmpty, let named = NSFont(name: config.family, size: fontSize)
        else { return NSFont.monospacedSystemFont(ofSize: fontSize, weight: bold ? .bold : .regular) }
        return bold ? boldVariant(of: named) : named
    }

    /// `font` with the bold trait, or `font` itself when the family has no bold face.
    static func boldVariant(of font: NSFont) -> NSFont {
        guard let derived = CTFontCreateCopyWithSymbolicTraits(
            font as CTFont, 0, nil, .traitBold, .traitBold)
        else { return font }
        // CTFont and NSFont are toll-free bridged.
        return derived as NSFont
    }

    /// The cell in points for a display of `scale`: a whole number of device pixels on each
    /// side, as kitty and Ghostty do. A fractional advance (SF Mono 13pt is 8.04pt) would make
    /// columns alternate between two widths and box drawing step by a pixel between
    /// neighbours. Grid size, mouse mapping, the IME rect and drawing all use this one size.
    static func cellSize(scale: CGFloat) -> CGSize {
        pixelAligned(naturalCellSize, scale: scale)
    }

    /// `size` rounded to whole device pixels at `scale`, never below one pixel.
    static func pixelAligned(_ size: CGSize, scale: CGFloat) -> CGSize {
        let scale = scale > 0 ? scale : 1
        return CGSize(
            width: max((size.width * scale).rounded(), 1) / scale,
            height: max((size.height * scale).rounded(), 1) / scale)
    }

    // MARK: - glyph rasterization

    /// Cell geometry in device pixels for the Metal renderer.
    static func gridMetrics(scale: CGFloat) -> GridMetrics {
        let font = font(bold: false)
        let size = cellSize(scale: scale)
        let descent = CTFontGetDescent(font)
        // The baseline sits one descender above the cell's bottom edge, on a whole pixel so
        // every row's glyphs fall on the pixel grid the same way.
        let baseline = ((size.height - descent) * scale).rounded()
        let thickness = max(CTFontGetUnderlineThickness(font), 1) * scale
        return GridMetrics(
            cellWidth: Float((size.width * scale).rounded()),
            cellHeight: Float((size.height * scale).rounded()),
            baseline: Float(baseline),
            lineThickness: Float(thickness.rounded()),
            strikeOffset: Float(((size.height - descent) - CTFontGetXHeight(font) / 2) * scale),
            scale: Float(scale)
        )
    }

    /// A rasterized glyph: `width`×`height` texels, one byte per texel for a mask glyph,
    /// BGRA premultiplied for a colour one. The bitmap covers the glyph's ink, not the cell;
    /// `bearingX`/`bearingY` place its top-left corner relative to the cell's, in pixels.
    struct GlyphBitmap {
        var width: Int
        var height: Int
        var colored: Bool
        var bytes: [UInt8]
        var bearingX = 0
        var bearingY = 0
    }

    /// Where a glyph's pen goes and how much it is shrunk, in points from the cell's top-left.
    private struct Placement {
        var penX: CGFloat
        var baseline: CGFloat
        var factor: CGFloat
    }

    /// Rasterizes one glyph (`span` columns wide) into a bitmap sized to its ink, so italic
    /// overhang, descenders and accents above the cap height are never cropped. Characters the
    /// font lacks go through CoreText's fallback cascade, so CJK and emoji come from the
    /// system's fonts; fallback, symbol (Nerd Font / Powerline) and colour glyphs larger than
    /// their cells are scaled down to fit them.
    static func rasterize(_ key: GlyphKey, metrics: GridMetrics) -> GlyphBitmap? {
        let (cellWidth, cellHeight) = metrics.glyphSize(span: Int(key.span))
        guard cellWidth > 0, cellHeight > 0 else { return nil }
        let base = face(bold: key.bold, italic: key.italic)
        let ink: CGRect
        let colored: Bool
        let fit: Bool
        let paint: (CGContext) -> Void
        if key.glyph != 0 {
            // A shaped glyph of the primary face (`shapeLigatures`): drawn at its own size so
            // the pieces of a ligature meet across cells.
            var glyph = CGGlyph(key.glyph)
            var bounds = CGRect.zero
            CTFontGetBoundingRectsForGlyphs(base, .default, &glyph, &bounds, 1)
            ink = bounds
            colored = false
            fit = false
            paint = { context in
                var position = CGPoint.zero
                CTFontDrawGlyphs(base, &glyph, &position, 1, context)
            }
        } else {
            guard let scalar = Unicode.Scalar(key.codepoint) else { return nil }
            let line = CTLineCreateWithAttributedString(attributed(key.text ?? String(Character(scalar)), base))
            let traits = runTraits(line, primary: base)
            ink = inkBounds(line, colored: traits.colored)
            colored = traits.colored
            fit = traits.colored || traits.fallback || isSymbol(key.codepoint)
            paint = { context in
                context.textMatrix = .identity
                context.textPosition = .zero
                CTLineDraw(line, context)
            }
        }
        let scale = CGFloat(metrics.scale)
        guard !ink.isNull, ink.width > 0, ink.height > 0 else { return nil }
        let place = placement(
            ink: ink,
            cell: CGSize(width: CGFloat(cellWidth) / scale, height: CGFloat(cellHeight) / scale),
            baseline: CGFloat(metrics.baseline) / scale,
            fit: fit)
        // The ink box in device pixels from the cell's top-left (y down), with a one-pixel
        // margin for antialiasing.
        let left = ((place.penX + ink.minX * place.factor) * scale).rounded(.down) - 1
        let right = ((place.penX + ink.maxX * place.factor) * scale).rounded(.up) + 1
        let top = ((place.baseline - ink.maxY * place.factor) * scale).rounded(.down) - 1
        let bottom = ((place.baseline - ink.minY * place.factor) * scale).rounded(.up) + 1
        let width = Int(right - left)
        let height = Int(bottom - top)
        // A runaway glyph (a font's broken bounding box) must not allocate a huge bitmap; a
        // ligature's last glyph may reach several cells back over its run.
        guard width > 0, height > 0, width <= cellWidth * 8, height <= cellHeight * 4
        else { return nil }
        let bytesPerRow = colored ? width * 4 : width
        var bytes = [UInt8](repeating: 0, count: bytesPerRow * height)
        let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
            let context: CGContext?
            if colored {
                context = CGContext(
                    data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                    bytesPerRow: bytesPerRow, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue
                        | CGBitmapInfo.byteOrder32Little.rawValue)
            } else {
                context = CGContext(
                    data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
                    bytesPerRow: bytesPerRow, space: CGColorSpaceCreateDeviceGray(),
                    bitmapInfo: CGImageAlphaInfo.none.rawValue)
            }
            guard let context else { return false }
            context.setAllowsFontSmoothing(false)
            context.setShouldSubpixelPositionFonts(true)
            if !colored { context.setFillColor(gray: 1, alpha: 1) }
            // The bitmap's origin is bottom-left; the pen sits at `penX`, `baseline` below
            // the cell's top, both measured from the bitmap's own corner.
            context.translateBy(
                x: place.penX * scale - left,
                y: CGFloat(height) - (place.baseline * scale - top))
            context.scaleBy(x: scale * place.factor, y: scale * place.factor)
            paint(context)
            return true
        }
        guard drawn else { return nil }
        return GlyphBitmap(
            width: width, height: height, colored: colored, bytes: bytes,
            bearingX: Int(left), bearingY: Int(top))
    }

    /// The primary face in a style.
    private static func face(bold: Bool, italic: Bool) -> CTFont {
        let base = font(bold: bold) as CTFont
        guard italic else { return base }
        return CTFontCreateCopyWithSymbolicTraits(base, 0, nil, .traitItalic, .traitItalic) ?? base
    }

    private static func attributed(_ text: String, _ font: CTFont) -> NSAttributedString {
        NSAttributedString(
            string: text,
            attributes: [
                NSAttributedString.Key(kCTFontAttributeName as String): font,
                // Mask glyphs are white on black; the GPU tints them with the cell's colour.
                NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true,
            ])
    }

    // MARK: - ligatures

    private struct ShapeKey: Hashable {
        var text: String
        var bold: Bool
        var italic: Bool
    }

    /// Shaped runs, keyed by run text and style; `[]` remembers "shaping changes nothing".
    /// Shared by every render thread, hence the lock.
    nonisolated(unsafe) private static var shaped: [ShapeKey: [UInt16]] = [:]
    private static let shapedLock = NSLock()

    /// One glyph ID per character of `text` (an ASCII run, one cell per character) after
    /// shaping with the font's ligatures and contextual alternates. `GlyphKey.noGlyph` marks
    /// a cell a multi-character ligature to its left covers. Nil when shaping draws the same
    /// glyphs as the characters alone (most runs, and every font without ligatures), when
    /// CoreText fell back to another font, or for non-ASCII text.
    static func shapeLigatures(_ text: String, bold: Bool, italic: Bool) -> [UInt16]? {
        let key = ShapeKey(text: text, bold: bold, italic: italic)
        shapedLock.lock()
        let hit = shaped[key]
        shapedLock.unlock()
        if let hit { return hit.isEmpty ? nil : hit }
        let result = shape(text, face(bold: bold, italic: italic)) ?? []
        shapedLock.lock()
        // A screen of changing output must not grow this without bound.
        if shaped.count > 4096 { shaped = [:] }
        shaped[key] = result
        shapedLock.unlock()
        return result.isEmpty ? nil : result
    }

    /// `shapeLigatures` against an explicit font, uncached (tests).
    static func shape(_ text: String, in font: NSFont) -> [UInt16]? {
        shape(text, font as CTFont)
    }

    private static func shape(_ text: String, _ font: CTFont) -> [UInt16]? {
        let units = Array(text.utf16)
        guard units.count >= 2, units.allSatisfy({ $0 > 0x20 && $0 < 0x7F }) else { return nil }
        var nominal = [CGGlyph](repeating: 0, count: units.count)
        guard CTFontGetGlyphsForCharacters(font, units, &nominal, units.count) else { return nil }
        let line = CTLineCreateWithAttributedString(attributed(text, font))
        guard let runs = CTLineGetGlyphRuns(line) as? [CTRun] else { return nil }
        let primaryName = CTFontCopyPostScriptName(font) as String
        var out = [UInt16](repeating: GlyphKey.noGlyph, count: units.count)
        for run in runs {
            let attributes = CTRunGetAttributes(run) as NSDictionary
            guard let value = attributes[kCTFontAttributeName as String],
                  CFGetTypeID(value as CFTypeRef) == CTFontGetTypeID(),
                  CTFontCopyPostScriptName(unsafeBitCast(value as AnyObject, to: CTFont.self)) as String
                    == primaryName
            else { return nil }
            let count = CTRunGetGlyphCount(run)
            var glyphs = [CGGlyph](repeating: 0, count: count)
            var indices = [CFIndex](repeating: 0, count: count)
            CTRunGetGlyphs(run, CFRange(location: 0, length: count), &glyphs)
            CTRunGetStringIndices(run, CFRange(location: 0, length: count), &indices)
            for (glyph, index) in zip(glyphs, indices) where index >= 0 && index < units.count {
                // Two glyphs for one character (a decomposition) would not fit one cell.
                guard out[index] == GlyphKey.noGlyph, glyph != 0 else { return nil }
                out[index] = glyph
            }
        }
        guard out[0] != GlyphKey.noGlyph, out != nominal.map({ UInt16($0) }) else { return nil }
        return out
    }

    /// Private-use (Nerd Font icons, Powerline) and the symbol/dingbat blocks: drawn by fonts
    /// at whatever size their designers liked, so they are fitted to the cell.
    private static func isSymbol(_ codepoint: UInt32) -> Bool {
        (0xE000...0xF8FF).contains(codepoint) || (0xF0000...0x10FFFF).contains(codepoint)
            || (0x2600...0x27BF).contains(codepoint) || (0x1F300...0x1FAFF).contains(codepoint)
    }

    /// The glyph's ink in points around the pen (y up). A colour (sbix) image can spill past
    /// the bounds CoreText reports for it, so colour glyphs also take the typographic box.
    private static func inkBounds(_ line: CTLine, colored: Bool) -> CGRect {
        var ink = CTLineGetBoundsWithOptions(line, .useGlyphPathBounds)
        if ink.isNull || ink.width <= 0 || ink.height <= 0 { ink = CTLineGetImageBounds(line, nil) }
        guard colored else { return ink }
        return ink.union(CTLineGetBoundsWithOptions(line, []))
    }

    /// Where to draw a glyph whose ink is `ink` in a cell of `cell` points. A glyph from the
    /// primary font keeps its own size and pen (its overhang just spills into the bitmap's
    /// margin). One that must `fit` and does not is shrunk to the cell and centred.
    private static func placement(ink: CGRect, cell: CGSize, baseline: CGFloat, fit: Bool) -> Placement {
        let top = baseline - ink.maxY
        let bottom = baseline - ink.minY
        let fits = ink.minX >= -0.5 && ink.maxX <= cell.width + 0.5 && top >= -0.5
            && bottom <= cell.height + 0.5
        guard fit, !fits else { return Placement(penX: 0, baseline: baseline, factor: 1) }
        let factor = min(cell.width / ink.width, cell.height / ink.height, 1)
        let width = ink.width * factor
        let height = ink.height * factor
        let penX = (cell.width - width) / 2 - ink.minX * factor
        // Keep the glyph on its baseline when it fits vertically; otherwise centre it.
        var line = baseline
        if baseline - ink.maxY * factor < 0 || baseline - ink.minY * factor > cell.height {
            line = (cell.height - height) / 2 + ink.maxY * factor
        }
        return Placement(penX: penX, baseline: line, factor: factor)
    }

    /// Whether CoreText drew `line` with a colour font (Apple Color Emoji and friends), and
    /// whether it fell back to a font other than `primary`.
    private static func runTraits(_ line: CTLine, primary: CTFont) -> (colored: Bool, fallback: Bool) {
        guard let runs = CTLineGetGlyphRuns(line) as? [CTRun] else { return (false, false) }
        let primaryName = CTFontCopyPostScriptName(primary) as String
        var colored = false
        var fallback = false
        for run in runs {
            let attributes = CTRunGetAttributes(run) as NSDictionary
            guard let value = attributes[kCTFontAttributeName as String],
                  CFGetTypeID(value as CFTypeRef) == CTFontGetTypeID()
            else { continue }
            // The type ID check above makes this cast unconditional.
            let runFont = unsafeBitCast(value as AnyObject, to: CTFont.self)
            if CTFontGetSymbolicTraits(runFont).contains(.traitColorGlyphs) { colored = true }
            if CTFontCopyPostScriptName(runFont) as String != primaryName { fallback = true }
        }
        return (colored, fallback)
    }
}
