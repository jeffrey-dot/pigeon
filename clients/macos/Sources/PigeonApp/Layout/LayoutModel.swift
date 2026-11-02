import AppKit
import PigeonCore

/// What the core says the split looks like. Geometry comes from the ABI; this
/// type only maps it to points and answers "what is under the mouse".
struct LayoutModel: Equatable {
    struct Pane: Equatable {
        var id: UInt32
        var rect: CGRect
    }

    struct Divider: Equatable {
        var splitID: UInt32
        var axis: PgAxis
        var ratio: Double
        /// The box this split divides (normalized to the container).
        var box: CGRect
    }

    var panes: [Pane]
    var dividers: [Divider]
    var focus: UInt32
    /// The pane filling the window, or 0 when every pane shows (`layout.zoom`).
    var zoom: UInt32

    static let empty = LayoutModel(panes: [], dividers: [], focus: 0)

    /// Reads a `layout` message through the core's accessors.
    init?(message: OpaquePointer) {
        var rects = [PgRect](repeating: Self.blankRect, count: 64)
        let rectCount = rects.withUnsafeMutableBufferPointer { buffer in
            pg_msg_layout_rects(message, buffer.baseAddress, buffer.count)
        }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(
                Data("pigeon: layout message rects=\(rectCount)\n".utf8)
            )
        }
        if rectCount == 0 {
            // `{leaf: 0}` means "no panes left"; keep the model empty so the
            // container removes the stale views instead of showing ghosts.
            self = .empty
            return
        }
        var splits = [PgSplit](repeating: Self.blankSplit, count: 64)
        let splitCount = splits.withUnsafeMutableBufferPointer { buffer in
            pg_msg_layout_splits(message, buffer.baseAddress, buffer.count)
        }
        self.init(
            panes: rects[0..<rectCount].map {
                Pane(
                    id: $0.pane_id,
                    rect: CGRect(x: Double($0.x), y: Double($0.y), width: Double($0.w), height: Double($0.h))
                )
            },
            dividers: splits[0..<splitCount].map {
                Divider(
                    splitID: $0.id,
                    axis: $0.axis,
                    ratio: Double($0.ratio),
                    box: CGRect(x: Double($0.x), y: Double($0.y), width: Double($0.w), height: Double($0.h))
                )
            },
            focus: pg_msg_layout_focus(message),
            zoom: pg_msg_layout_zoom(message)
        )
    }

    init(panes: [Pane], dividers: [Divider], focus: UInt32, zoom: UInt32 = 0) {
        self.panes = panes
        self.dividers = dividers
        self.focus = focus
        self.zoom = zoom
    }

    private static let blankRect = PgRect(
        size: UInt32(MemoryLayout<PgRect>.size), pane_id: 0, x: 0, y: 0, w: 0, h: 0
    )
    private static let blankSplit = PgSplit(
        size: UInt32(MemoryLayout<PgSplit>.size), id: 0, axis: PG_AXIS_VERTICAL,
        ratio: 0, x: 0, y: 0, w: 0, h: 0
    )

    // MARK: - hit testing (pure, in normalized coordinates)

    /// The divider within `tolerance` of a normalized point, if any.
    func divider(at point: CGPoint, tolerance: Double) -> Divider? {
        visibleDividers.first { divider in
            switch divider.axis {
            case PG_AXIS_VERTICAL:
                let x = divider.box.minX + divider.box.width * divider.ratio
                return abs(point.x - x) <= tolerance
                    && point.y >= divider.box.minY && point.y <= divider.box.maxY
            default:
                let y = divider.box.minY + divider.box.height * divider.ratio
                return abs(point.y - y) <= tolerance
                    && point.x >= divider.box.minX && point.x <= divider.box.maxX
            }
        }
    }

    /// The pane under a normalized point.
    func pane(at point: CGPoint) -> Pane? {
        // Zoomed: the siblings keep their rects (for unzoom) but are hidden under the zoomed
        // pane, so a click must not focus one of them.
        if zoom != 0 { return panes.first { $0.id == zoom && $0.rect.contains(point) } }
        return panes.first { $0.rect.contains(point) }
    }

    /// Dividers that are on screen: none while a pane fills the window, so they are neither
    /// drawn over it nor dragged through it.
    var visibleDividers: [Divider] { zoom != 0 ? [] : dividers }

    /// Ratio a drag would produce, in normalized box coordinates.
    func proposedRatio(_ divider: Divider, at point: CGPoint) -> Double {
        switch divider.axis {
        case PG_AXIS_VERTICAL:
            guard divider.box.width > 0 else { return divider.ratio }
            return Double((point.x - divider.box.minX) / divider.box.width)
        default:
            guard divider.box.height > 0 else { return divider.ratio }
            return Double((point.y - divider.box.minY) / divider.box.height)
        }
    }

    /// Panes in tree order (left-to-right, top-to-bottom as the core reports).
    var orderedPanes: [UInt32] { panes.map(\.id) }

    /// Next pane in tree order, wrapping — same order the core uses.
    func nextFocus(forward: Bool) -> UInt32? {
        let ids = orderedPanes
        guard !ids.isEmpty else { return nil }
        let index = ids.firstIndex(of: focus) ?? 0
        let next = forward ? (index + 1) % ids.count : (index + ids.count - 1) % ids.count
        return ids[next]
    }
}
