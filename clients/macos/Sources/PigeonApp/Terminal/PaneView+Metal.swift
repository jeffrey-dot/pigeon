import AppKit

/// How `PaneView` gets on screen: turns the cells the pane holds into `GridFrame`s for its
/// render thread. Colours and geometry are resolved here, on the main thread, because
/// dynamic `NSColor`s only resolve against the view's appearance.
extension PaneView {
    /// Starts or stops the renderer to match visibility: a hidden pane (a zoomed-out
    /// sibling, a background session) keeps no thread, instance buffers or atlas.
    func syncRenderer() {
        let visible = window != nil && !isHiddenOrHasHiddenAncestor
        if visible, metal == nil {
            guard let renderer = MetalGridRenderer() else {
                // Every Mac that runs macOS 13 has a Metal device; this is a broken install
                // (no device, shaders that do not compile), and the pane stays blank.
                FileHandle.standardError.write(Data("pigeon: no Metal renderer for this pane\n".utf8))
                return
            }
            renderer.layer.contentsScale = window?.backingScaleFactor ?? 2
            // Layer-hosting: AppKit leaves the layer to us and stops calling `draw(_:)`.
            layer = renderer.layer
            wantsLayer = true
            renderer.start()
            metal = renderer
            dirty = .everything
            postFrame()
        } else if !visible, let renderer = metal {
            renderer.stop()
            metal = nil
        }
    }

    /// Hands the current screen to the render thread. Cheap: the cell array is shared
    /// copy-on-write, and the thread rebuilds only the rows in `dirty`.
    func postFrame() {
        guard let metal, bounds.width > 0, bounds.height > 0 else { return }
        if FrameStats.enabled {
            // Frame rates of an occluded window are throttled by the system; benchmarks
            // (`tools/bench/render-bench.sh`) need to know which numbers they are looking at.
            let visible = window?.occlusionState.contains(.visible) ?? false
            if visible != reportedVisible {
                reportedVisible = visible
                FileHandle.standardError.write(Data("pigeon: window visible=\(visible)\n".utf8))
            }
        }
        let scale = window?.backingScaleFactor ?? 2
        let appearance = effectiveAppearance
        let palette = CellPalette.resolve(for: appearance)
        var thumb: (SIMD2<Float>, SIMD2<Float>)?
        if let rect = Self.scrollThumb(
            bounds: bounds, rows: rows, history: scrollHistory, offset: scrollOffset) {
            // `scrollThumb` is in AppKit's bottom-left space; the GPU draws top-left.
            thumb = (
                SIMD2(Float(rect.minX * scale), Float((bounds.height - rect.maxY) * scale)),
                SIMD2(Float(rect.width * scale), Float(rect.height * scale)))
        }
        let chrome = OverlayColors.resolve(for: appearance)
        var shown = (cells: cells, cursor: cursor)
        shown.cursor.hollow = !hasKeyboardFocus
        if blinkTimer != nil, !blink.isLit(at: Date()) { shown.cursor.visible = false }
        var overlayRow: Int?
        if let preedit, let overlay = preedit.overlay(cells: cells, cursor: cursor, cols: cols, rows: rows) {
            shown = (overlay.cells, overlay.cursor)
            overlayRow = overlay.row
        }
        // Only the row the composing overlay left and the row it covers differ from the grid.
        for row in Preedit.rowsToRedraw(previous: preeditRow, current: overlayRow, rows: rows) {
            dirty.mark(row)
        }
        preeditRow = overlayRow
        metal.layer.contentsScale = scale
        metal.layer.backgroundColor = CellPalette.color(palette.background).cgColor
        metal.post(GridFrame(
            cells: shown.cells,
            cols: cols,
            rows: rows,
            cursor: shown.cursor,
            palette: palette,
            metrics: CoreTextRenderer.gridMetrics(scale: scale),
            width: Int((bounds.width * scale).rounded()),
            height: Int((bounds.height * scale).rounded()),
            thumb: thumb.map { (origin: $0.0, size: $0.1) },
            thumbColor: chrome.thumb,
            dirty: dirty,
            selection: selectionSpans(),
            selectionColor: chrome.selection))
        dirty = .none
    }

    /// Schedules one post for the end of this run-loop turn. A burst of deltas (one socket
    /// read can carry dozens) then patches `cells` in place: posting after each would leave
    /// the mailbox sharing the array, and the next patch would copy the whole grid.
    /// A hidden pane has no renderer and catches up with one full frame when it becomes
    /// visible again (`dirty` stays `.everything` until then).
    func screenChanged() {
        guard !postScheduled else { return }
        postScheduled = true
        RunLoop.main.perform(inModes: [.common]) { [weak self] in
            guard let self else { return }
            postScheduled = false
            postFrame()
        }
    }
}

/// The scroll thumb and selection colours for one appearance. Resolving a dynamic `NSColor`
/// means switching the drawing appearance, which every posted frame used to do twice.
struct OverlayColors {
    var thumb: SIMD4<Float>
    var selection: SIMD4<Float>

    nonisolated(unsafe) private static var cache: [NSAppearance.Name: OverlayColors] = [:]

    /// The accent colour is a user setting: a change in System Settings drops the cache.
    nonisolated(unsafe) private static let observer = NotificationCenter.default.addObserver(
        forName: NSColor.systemColorsDidChangeNotification, object: nil, queue: .main
    ) { _ in cache = [:] }

    /// Main thread only (it reads AppKit colours), so the cache needs no lock.
    static func resolve(for appearance: NSAppearance) -> OverlayColors {
        _ = observer
        if let hit = cache[appearance.name] { return hit }
        var colors = OverlayColors(
            thumb: SIMD4(0.5, 0.5, 0.5, 0.45), selection: SIMD4(0.3, 0.5, 1, 0.38))
        appearance.performAsCurrentDrawingAppearance {
            let thumb = NSColor.secondaryLabelColor.usingColorSpace(.sRGB) ?? .gray
            colors.thumb = SIMD4(
                Float(thumb.redComponent), Float(thumb.greenComponent),
                Float(thumb.blueComponent), Float(thumb.alphaComponent) * 0.45)
            // The system's selection colour, translucent so the text under it stays readable.
            let accent = NSColor.selectedContentBackgroundColor.usingColorSpace(.sRGB) ?? .systemBlue
            colors.selection = SIMD4(
                Float(accent.redComponent), Float(accent.greenComponent),
                Float(accent.blueComponent), 0.38)
        }
        cache[appearance.name] = colors
        return colors
    }
}
