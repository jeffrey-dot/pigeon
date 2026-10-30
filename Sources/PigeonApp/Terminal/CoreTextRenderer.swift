import AppKit
import CoreText

/// Font metrics and glyph rasterization for the Metal grid renderer. The only file allowed
/// to import CoreText; the grid itself is drawn on the GPU (`MetalGridRenderer`).
enum CoreTextRenderer {
    static let fontSize: CGFloat = 13
    /// harp's terminal line-height ratio (`DEFAULT_UI_LINE_HEIGHT_RATIO` in
    /// `crates/warpui_core/src/elements/gui/text.rs:33`, used as the terminal default).
    static let lineHeightRatio: CGFloat = 1.2
    /// harp ships "Hack" (`app/src/settings/font.rs:11`); when it is not installed we keep the
    /// system monospace rather than bundling a font file.
    static let fontName = "Hack"

    static func font(bold: Bool) -> NSFont {
        if let named = NSFont(name: bold ? "\(fontName)-Bold" : fontName, size: fontSize) {
            return named
        }
        return NSFont.monospacedSystemFont(ofSize: fontSize, weight: bold ? .bold : .regular)
    }

    static func cellSize() -> CGSize {
        let font = font(bold: false)
        let advance = ("M" as NSString).size(withAttributes: [.font: font])
        // Cell height is the font size times harp's line-height ratio, not the font's own
        // ascender/descender span: that is what gives harp's rows their airiness.
        return CGSize(width: max(advance.width, 1), height: max(fontSize * lineHeightRatio, 1))
    }

    // MARK: - glyph rasterization

    /// Cell geometry in device pixels for the Metal renderer.
    static func gridMetrics(scale: CGFloat) -> GridMetrics {
        let font = font(bold: false)
        let size = cellSize()
        let descent = CTFontGetDescent(font)
        // The baseline sits one descender above the cell's bottom edge.
        let baseline = size.height - descent
        let thickness = max(CTFontGetUnderlineThickness(font), 1) * scale
        return GridMetrics(
            cellWidth: Float(size.width * scale),
            cellHeight: Float(size.height * scale),
            baseline: Float(baseline * scale),
            lineThickness: Float(thickness.rounded()),
            strikeOffset: Float(((size.height - descent) - CTFontGetXHeight(font) / 2) * scale),
            scale: Float(scale)
        )
    }

    /// A rasterized glyph: `width`×`height` texels, one byte per texel for a mask glyph,
    /// BGRA premultiplied for a colour one.
    struct GlyphBitmap {
        var width: Int
        var height: Int
        var colored: Bool
        var bytes: [UInt8]
    }

    /// Rasterizes one glyph into a cell-sized bitmap (`span` columns wide), baseline placed
    /// where the grid expects it. Characters the font lacks go through CoreText's fallback
    /// cascade, so CJK and emoji come from the system's fonts.
    static func rasterize(_ key: GlyphKey, metrics: GridMetrics) -> GlyphBitmap? {
        guard let scalar = Unicode.Scalar(key.codepoint) else { return nil }
        let (width, height) = metrics.glyphSize(span: Int(key.span))
        guard width > 0, height > 0 else { return nil }
        var base = font(bold: key.bold) as CTFont
        if key.italic {
            base = CTFontCreateCopyWithSymbolicTraits(base, 0, nil, .traitItalic, .traitItalic) ?? base
        }
        let attributed = NSAttributedString(
            string: String(Character(scalar)),
            attributes: [
                NSAttributedString.Key(kCTFontAttributeName as String): base,
                // Mask glyphs are white on black; the GPU tints them with the cell's colour.
                NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true,
            ]
        )
        let line = CTLineCreateWithAttributedString(attributed)
        let colored = lineIsColored(line)
        let scale = CGFloat(metrics.scale)
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
            context.scaleBy(x: scale, y: scale)
            if !colored { context.setFillColor(gray: 1, alpha: 1) }
            context.textMatrix = .identity
            // The bitmap's origin is bottom-left; the baseline sits `baseline` below the top.
            let cellHeight = CGFloat(height) / scale
            context.textPosition = CGPoint(x: 0, y: cellHeight - CGFloat(metrics.baseline) / scale)
            CTLineDraw(line, context)
            return true
        }
        guard drawn else { return nil }
        return GlyphBitmap(width: width, height: height, colored: colored, bytes: bytes)
    }

    /// True when the run CoreText picked is a colour font (Apple Color Emoji and friends).
    private static func lineIsColored(_ line: CTLine) -> Bool {
        guard let runs = CTLineGetGlyphRuns(line) as? [CTRun] else { return false }
        for run in runs {
            let attributes = CTRunGetAttributes(run) as NSDictionary
            guard let value = attributes[kCTFontAttributeName as String],
                  CFGetTypeID(value as CFTypeRef) == CTFontGetTypeID()
            else { continue }
            // The type ID check above makes this cast unconditional.
            let runFont = unsafeBitCast(value as AnyObject, to: CTFont.self)
            if CTFontGetSymbolicTraits(runFont).contains(.traitColorGlyphs) { return true }
        }
        return false
    }
}
