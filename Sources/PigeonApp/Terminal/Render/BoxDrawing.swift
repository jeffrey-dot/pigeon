/// Box-drawing (U+2500–257F) and block elements (U+2580–259F) drawn as rectangles instead of
/// font glyphs. A font's `─` rarely spans the full cell at every size and line height, so
/// TUI borders break into dashes; rectangles computed from the cell always meet.
enum BoxDrawing {
    /// Stroke weight of one arm from the cell centre.
    private enum Weight: UInt8 { case none = 0, light, heavy, double }

    /// Arms as (left, right, up, down).
    private static let arms: [UInt32: (Weight, Weight, Weight, Weight)] = {
        var table: [UInt32: (Weight, Weight, Weight, Weight)] = [:]
        let rows: [(UInt32, UInt8, UInt8, UInt8, UInt8)] = [
            (0x2500, 1, 1, 0, 0), (0x2501, 2, 2, 0, 0), (0x2502, 0, 0, 1, 1), (0x2503, 0, 0, 2, 2),
            (0x250C, 0, 1, 0, 1), (0x250D, 0, 2, 0, 1), (0x250E, 0, 1, 0, 2), (0x250F, 0, 2, 0, 2),
            (0x2510, 1, 0, 0, 1), (0x2511, 2, 0, 0, 1), (0x2512, 1, 0, 0, 2), (0x2513, 2, 0, 0, 2),
            (0x2514, 0, 1, 1, 0), (0x2515, 0, 2, 1, 0), (0x2516, 0, 1, 2, 0), (0x2517, 0, 2, 2, 0),
            (0x2518, 1, 0, 1, 0), (0x2519, 2, 0, 1, 0), (0x251A, 1, 0, 2, 0), (0x251B, 2, 0, 2, 0),
            (0x251C, 0, 1, 1, 1), (0x2523, 0, 2, 2, 2), (0x2524, 1, 0, 1, 1), (0x252B, 2, 0, 2, 2),
            (0x252C, 1, 1, 0, 1), (0x2533, 2, 2, 0, 2), (0x2534, 1, 1, 1, 0), (0x253B, 2, 2, 2, 0),
            (0x253C, 1, 1, 1, 1), (0x254B, 2, 2, 2, 2),
            (0x2550, 3, 3, 0, 0), (0x2551, 0, 0, 3, 3), (0x2554, 0, 3, 0, 3), (0x2557, 3, 0, 0, 3),
            (0x255A, 0, 3, 3, 0), (0x255D, 3, 0, 3, 0), (0x2560, 0, 3, 3, 3), (0x2563, 3, 0, 3, 3),
            (0x2566, 3, 3, 0, 3), (0x2569, 3, 3, 3, 0), (0x256C, 3, 3, 3, 3),
            // Mixed single/double junctions, as in `╞══╪══╡` table rules.
            (0x2552, 0, 3, 0, 1), (0x2553, 0, 1, 0, 3), (0x2555, 3, 0, 0, 1), (0x2556, 1, 0, 0, 3),
            (0x2558, 0, 3, 1, 0), (0x2559, 0, 1, 3, 0), (0x255B, 3, 0, 1, 0), (0x255C, 1, 0, 3, 0),
            (0x255E, 0, 3, 1, 1), (0x255F, 0, 1, 3, 3), (0x2561, 3, 0, 1, 1), (0x2562, 1, 0, 3, 3),
            (0x2564, 3, 3, 0, 1), (0x2565, 1, 1, 0, 3), (0x2567, 3, 3, 1, 0), (0x2568, 1, 1, 3, 0),
            (0x256A, 3, 3, 1, 1), (0x256B, 1, 1, 3, 3),
            // Rounded corners are drawn square; at terminal sizes the difference is a pixel.
            (0x256D, 0, 1, 0, 1), (0x256E, 1, 0, 0, 1), (0x256F, 1, 0, 1, 0), (0x2570, 0, 1, 1, 0),
            (0x2574, 1, 0, 0, 0), (0x2575, 0, 0, 1, 0), (0x2576, 0, 1, 0, 0), (0x2577, 0, 0, 0, 1),
            (0x2578, 2, 0, 0, 0), (0x2579, 0, 0, 2, 0), (0x257A, 0, 2, 0, 0), (0x257B, 0, 0, 0, 2),
        ]
        for (code, left, right, up, down) in rows {
            table[code] = (
                Weight(rawValue: left) ?? .none, Weight(rawValue: right) ?? .none,
                Weight(rawValue: up) ?? .none, Weight(rawValue: down) ?? .none)
        }
        return table
    }()

    /// Whether `codepoint` is drawn here rather than looked up in the font.
    static func handles(_ codepoint: UInt32) -> Bool {
        arms[codepoint] != nil || (0x2580...0x2593).contains(codepoint)
    }

    /// The rectangles for `codepoint` in the cell at `origin` of `size`, or nil when the font
    /// should draw it. `thin` is the light stroke in pixels.
    static func rects(
        _ codepoint: UInt32, origin: SIMD2<Float>, size: SIMD2<Float>, thin: Float,
        color: SIMD4<Float>
    ) -> [RectInstance]? {
        if let block = block(codepoint, origin: origin, size: size, color: color) { return [block] }
        guard let (left, right, up, down) = arms[codepoint] else { return nil }
        let light = max(thin.rounded(), 1)
        let centre = SIMD2(
            (origin.x + size.x / 2 - light / 2).rounded(.down),
            (origin.y + size.y / 2 - light / 2).rounded(.down))
        var out: [RectInstance] = []
        func stroke(_ weight: Weight) -> (width: Float, offsets: [Float]) {
            switch weight {
            case .none: return (0, [])
            case .light: return (light, [0])
            case .heavy: return (light * 2, [-light / 2])
            case .double: return (light, [-light, light])
            }
        }
        // Each arm runs from the cell edge to just past the centre, so arms meet and a
        // neighbouring cell's arm continues the line without a gap.
        func horizontal(_ weight: Weight, toLeft: Bool) {
            let (width, offsets) = stroke(weight)
            for offset in offsets {
                let y = (centre.y + offset).rounded(.down)
                let start = toLeft ? origin.x : centre.x
                let end = toLeft ? centre.x + width : origin.x + size.x
                out.append(RectInstance(origin: SIMD2(start, y), size: SIMD2(end - start, width), color: color))
            }
        }
        func vertical(_ weight: Weight, toTop: Bool) {
            let (width, offsets) = stroke(weight)
            for offset in offsets {
                let x = (centre.x + offset).rounded(.down)
                let start = toTop ? origin.y : centre.y
                let end = toTop ? centre.y + width : origin.y + size.y
                out.append(RectInstance(origin: SIMD2(x, start), size: SIMD2(width, end - start), color: color))
            }
        }
        horizontal(left, toLeft: true)
        horizontal(right, toLeft: false)
        vertical(up, toTop: true)
        vertical(down, toTop: false)
        return out
    }

    /// Halves, eighths, the full block and the three shades.
    private static func block(
        _ codepoint: UInt32, origin: SIMD2<Float>, size: SIMD2<Float>, color: SIMD4<Float>
    ) -> RectInstance? {
        func rect(x: Float, y: Float, width: Float, height: Float, alpha: Float = 1) -> RectInstance {
            let left = (origin.x + x * size.x).rounded(.down)
            let top = (origin.y + y * size.y).rounded(.down)
            let right = (origin.x + (x + width) * size.x).rounded(.down)
            let bottom = (origin.y + (y + height) * size.y).rounded(.down)
            var tinted = color
            tinted.w *= alpha
            return RectInstance(origin: SIMD2(left, top), size: SIMD2(right - left, bottom - top), color: tinted)
        }
        switch codepoint {
        case 0x2580: return rect(x: 0, y: 0, width: 1, height: 0.5)
        case 0x2581...0x2588:
            let eighths = Float(codepoint - 0x2580) / 8
            return rect(x: 0, y: 1 - eighths, width: 1, height: eighths)
        case 0x2589...0x258F:
            let eighths = Float(0x2590 - codepoint) / 8
            return rect(x: 0, y: 0, width: eighths, height: 1)
        case 0x2590: return rect(x: 0.5, y: 0, width: 0.5, height: 1)
        case 0x2591: return rect(x: 0, y: 0, width: 1, height: 1, alpha: 0.25)
        case 0x2592: return rect(x: 0, y: 0, width: 1, height: 1, alpha: 0.5)
        case 0x2593: return rect(x: 0, y: 0, width: 1, height: 1, alpha: 0.75)
        default: return nil
        }
    }
}
