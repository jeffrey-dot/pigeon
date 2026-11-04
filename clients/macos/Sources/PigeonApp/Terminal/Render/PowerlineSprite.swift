/// Powerline separators (U+E0B0–E0BF) drawn from the cell instead of the font, as kitty and
/// Ghostty do: a font's triangle is rarely exactly one cell tall at every line height, so
/// prompt segments show a sliver of background above or below the arrow.
///
/// Produces a cell-sized 8-bit coverage mask, so the glyph goes through the atlas and is
/// tinted like any other. Pure: no CoreText, testable without a GPU.
enum PowerlineSprite {
    static func handles(_ codepoint: UInt32) -> Bool {
        shape(codepoint) != nil
    }

    private enum Shape {
        /// A filled triangle pointing right (or left when mirrored).
        case triangle
        /// The triangle's outline: two strokes meeting at the tip.
        case chevron
        /// A filled half disc bulging right.
        case halfDisc
        /// The half disc's outline.
        case arc
        /// A filled right triangle with the slanted edge from one corner to the other.
        case wedge(upper: Bool)
        /// The diagonal stroke alone.
        case slash(upper: Bool)
    }

    /// The shape and whether it is mirrored left↔right.
    private static func shape(_ codepoint: UInt32) -> (Shape, mirrored: Bool)? {
        switch codepoint {
        case 0xE0B0: return (.triangle, false)
        case 0xE0B1: return (.chevron, false)
        case 0xE0B2: return (.triangle, true)
        case 0xE0B3: return (.chevron, true)
        case 0xE0B4: return (.halfDisc, false)
        case 0xE0B5: return (.arc, false)
        case 0xE0B6: return (.halfDisc, true)
        case 0xE0B7: return (.arc, true)
        // Lower-left / upper-left / lower-right / upper-right triangles.
        case 0xE0B8: return (.wedge(upper: false), false)
        case 0xE0B9: return (.slash(upper: false), false)
        case 0xE0BA: return (.wedge(upper: false), true)
        case 0xE0BB: return (.slash(upper: false), true)
        case 0xE0BC: return (.wedge(upper: true), false)
        case 0xE0BD: return (.slash(upper: true), false)
        case 0xE0BE: return (.wedge(upper: true), true)
        case 0xE0BF: return (.slash(upper: true), true)
        default: return nil
        }
    }

    /// Supersampling per axis: enough for smooth slanted edges at terminal sizes.
    private static let samples = 4

    /// A `width`×`height` coverage mask, or nil for a codepoint this does not draw.
    /// `stroke` is the outline weight in pixels for the chevron, arc and slash shapes.
    static func mask(_ codepoint: UInt32, width: Int, height: Int, stroke: Float) -> [UInt8]? {
        guard let (shape, mirrored) = shape(codepoint), width > 0, height > 0 else { return nil }
        let w = Float(width)
        let h = Float(height)
        let half = max(stroke, 1) / 2
        let inside: (Float, Float) -> Bool
        switch shape {
        case .triangle:
            // Tip at the right edge's midpoint.
            inside = { x, y in x <= w * (1 - abs(y - h / 2) / (h / 2)) }
        case .chevron:
            inside = { x, y in
                // Horizontal offset from the edge, turned into the perpendicular distance
                // to a line of slope dx/dy = 2w/h.
                let edge = w * (1 - abs(y - h / 2) / (h / 2))
                let slope = 2 * w / h
                return abs(x - edge) / (slope * slope + 1).squareRoot() <= half
            }
        case .halfDisc:
            inside = { x, y in
                let dy = (y - h / 2) / (h / 2)
                return x <= w * (1 - dy * dy).squareRoot()
            }
        case .arc:
            inside = { x, y in
                let dy = (y - h / 2) / (h / 2)
                guard abs(dy) <= 1 else { return false }
                return abs(x - w * (1 - dy * dy).squareRoot()) <= half
            }
        case let .wedge(upper):
            // Filled below (or above) the diagonal from one corner to the other.
            inside = { x, y in upper ? y <= h * (1 - x / w) : y >= h * x / w }
        case let .slash(upper):
            inside = { x, y in
                let line = upper ? h * (1 - x / w) : h * x / w
                let length = (w * w + h * h).squareRoot()
                return abs(y - line) * w / length <= half
            }
        }
        var out = [UInt8](repeating: 0, count: width * height)
        let step = 1 / Float(samples)
        let total = Float(samples * samples)
        for row in 0..<height {
            for column in 0..<width {
                var hits = 0
                for sy in 0..<samples {
                    for sx in 0..<samples {
                        var x = Float(column) + (Float(sx) + 0.5) * step
                        let y = Float(row) + (Float(sy) + 0.5) * step
                        if mirrored { x = w - x }
                        if inside(x, y) { hits += 1 }
                    }
                }
                out[row * width + column] = UInt8((Float(hits) / total * 255).rounded())
            }
        }
        return out
    }
}
