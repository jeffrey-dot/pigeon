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
            // (`tools/render-bench.sh`) need to know which numbers they are looking at.
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
        var thumbColor = SIMD4<Float>(0.5, 0.5, 0.5, 0.45)
        appearance.performAsCurrentDrawingAppearance {
            let color = NSColor.secondaryLabelColor.usingColorSpace(.sRGB) ?? .gray
            thumbColor = SIMD4(
                Float(color.redComponent), Float(color.greenComponent),
                Float(color.blueComponent), Float(color.alphaComponent) * 0.45)
        }
        var shown = (cells: cells, cursor: cursor)
        shown.cursor.hollow = !hasKeyboardFocus
        if blinkTimer != nil, !blink.isLit(at: Date()) { shown.cursor.visible = false }
        if let preedit, let overlay = preedit.overlay(cells: cells, cursor: cursor, cols: cols, rows: rows) {
            shown = (overlay.cells, overlay.cursor)
            dirty.mark(overlay.row)
        }
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
            thumbColor: thumbColor,
            dirty: dirty,
            selection: selectionSpans(),
            selectionColor: selectionColor(appearance)))
        dirty = .none
    }

    /// The system's selection colour, translucent so the text under it stays readable.
    private func selectionColor(_ appearance: NSAppearance) -> SIMD4<Float> {
        var color = SIMD4<Float>(0.3, 0.5, 1, 0.35)
        appearance.performAsCurrentDrawingAppearance {
            let accent = NSColor.selectedContentBackgroundColor.usingColorSpace(.sRGB) ?? .systemBlue
            color = SIMD4(
                Float(accent.redComponent), Float(accent.greenComponent),
                Float(accent.blueComponent), 0.38)
        }
        return color
    }

    /// Posts the current screen; a hidden pane has no renderer and catches up with one
    /// full frame when it becomes visible again (`dirty` stays `.everything` until then).
    func screenChanged() {
        postFrame()
    }
}
