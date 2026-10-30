import AppKit
import PigeonCore

/// The single terminal pane: shows a grid snapshot, sends keys, reports resizes.
final class PaneView: NSView {
    struct Cell: Sendable {
        var codepoint: UInt32
        var attrs: UInt8
        var fg: UInt32
        var bg: UInt32
        var width: UInt8

        static let blank = Cell(codepoint: 32, attrs: 0, fg: 0, bg: 0, width: 1)
    }

    struct Cursor: Sendable {
        var x: Int
        var y: Int
        var visible: Bool
    }

    /// Set by the container once the pane view is created.
    var paneID: UInt32?
    private(set) var cols = 80
    private(set) var rows = 24
    private(set) var cells: [Cell] = []
    /// The alternate screen keeps no scrollback: vim/htop/less own the whole screen.
    private(set) var alt = false
    /// Lines the user wants to look back (positive) or forward (negative). The host owns the
    /// history and answers with the shifted screen (`scroll`, protocol §3.3), so a ten
    /// thousand line backlog never travels over the wire.
    var onScroll: ((Int) -> Void)?
    /// `scroll { offset: 0 }`: snap back to the live screen.
    var onScrollToBottom: (() -> Void)?
    /// Where this view is in the scrollback, from the host's `scroll` answer: total history
    /// lines and how far back the screen is. Both 0 means "nothing to show".
    private(set) var scrollHistory = 0
    private(set) var scrollOffset = 0
    private(set) var cursor = Cursor(x: 0, y: 0, visible: false)

    static let rendererChoice = RendererChoice.fromEnvironment(
        ProcessInfo.processInfo.environment["PIGEON_RENDERER"])
    /// The GPU renderer while the pane is visible and `PIGEON_RENDERER=metal`; nil otherwise.
    var metal: MetalGridRenderer?
    /// Rows changed since the last frame was posted to `metal`.
    var dirty = DirtyRows.everything
    /// Draw times of the CoreText path, for comparing against Metal (`PIGEON_FRAME_STATS=1`).
    private var cpuStats = FrameStats(renderer: "coretext")
    /// Last occlusion state logged under `PIGEON_FRAME_STATS`.
    var reportedVisible: Bool?

    var onInput: (([UInt8]) -> Void)?
    var onResize: ((UInt16, UInt16) -> Void)?
    private var reported: (UInt16, UInt16) = (0, 0)

    override var acceptsFirstResponder: Bool { true }
    override var isOpaque: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(
                Data("pigeon: pane draw cells=\(cells.count) bounds=\(bounds)\n".utf8)
            )
        }
        let start = FrameStats.enabled ? DispatchTime.now().uptimeNanoseconds : 0
        CoreTextRenderer.draw(cells: cells, cols: cols, rows: rows, cursor: cursor, in: bounds)
        drawScrollIndicator()
        if FrameStats.enabled {
            cpuStats.record(cpuMicros: Double(DispatchTime.now().uptimeNanoseconds - start) / 1000)
            cpuStats.reportIfDue()
            // AppKit has no "went idle" hook; report what a short burst left a second later.
            if cpuStats.cpu.count == 1 {
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.1) { [weak self] in
                    self?.cpuStats.flush()
                }
            }
        }
    }

    /// Applies the position reported by a `scroll` answer; `total == 0` hides the indicator.
    func setScrollPosition(total: Int, offset: Int) {
        scrollHistory = max(total, 0)
        scrollOffset = min(max(offset, 0), scrollHistory)
        screenChanged()
    }

    /// The indicator thumb in view coordinates, or nil when there is no history. Offset 0 is
    /// the live screen, which is the bottom of the track.
    static func scrollThumb(
        bounds: CGRect, rows: Int, history: Int, offset: Int
    ) -> CGRect? {
        guard history > 0, rows > 0, bounds.height > 0, bounds.width > 8 else { return nil }
        let width: CGFloat = 3
        let visible = CGFloat(rows) / CGFloat(rows + history)
        let height = max(bounds.height * visible, 18)
        let travel = bounds.height - height
        let progress = CGFloat(min(max(offset, 0), history)) / CGFloat(history)
        return CGRect(
            x: bounds.maxX - width - 2, y: travel * progress, width: width, height: height)
    }

    private func drawScrollIndicator() {
        guard let thumb = Self.scrollThumb(
            bounds: bounds, rows: rows, history: scrollHistory, offset: scrollOffset)
        else { return }
        NSColor.secondaryLabelColor.withAlphaComponent(0.45).setFill()
        NSBezierPath(roundedRect: thumb, xRadius: 1.5, yRadius: 1.5).fill()
    }

    /// Wheel delta in whole lines; the sub-line remainder is carried to the next event so a
    /// trackpad swipe does not need a whole cell per event to move.
    static func wheelLines(
        deltaY: CGFloat,
        precise: Bool,
        cellHeight: CGFloat,
        remainder: CGFloat
    ) -> (lines: Int, remainder: CGFloat) {
        guard precise, cellHeight > 0 else {
            return (Int(deltaY.rounded()), 0)
        }
        let pixels = remainder + deltaY
        let lines = Int((pixels / cellHeight).rounded(.towardZero))
        return (lines, pixels - CGFloat(lines) * cellHeight)
    }

    private var wheelRemainder: CGFloat = 0

    /// What one wheel event should ask the host for. The alternate screen has no history to
    /// look at, so it asks for nothing and the wheel goes nowhere (like every terminal).
    static func scrollRequest(
        alt: Bool,
        deltaY: CGFloat,
        precise: Bool,
        cellHeight: CGFloat,
        remainder: CGFloat
    ) -> (lines: Int, remainder: CGFloat) {
        guard !alt else { return (0, remainder) }
        return wheelLines(
            deltaY: deltaY, precise: precise, cellHeight: cellHeight, remainder: remainder)
    }

    override func scrollWheel(with event: NSEvent) {
        let (lines, remainder) = Self.scrollRequest(
            alt: alt,
            deltaY: event.scrollingDeltaY,
            precise: event.hasPreciseScrollingDeltas,
            cellHeight: CoreTextRenderer.cellSize().height,
            remainder: wheelRemainder
        )
        wheelRemainder = remainder
        guard lines != 0 else { return }
        onScroll?(lines)
    }

    /// PageUp/PageDown look back through the history on the live screen; on the alternate
    /// screen the program (`less`, `vim`) wants them, so they keep going to the pane.
    private func page(_ direction: Int) -> Bool {
        guard !alt else { return false }
        onScroll?(direction * max(rows - 1, 1))
        return true
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        // The grid's default fg/bg come from the theme, so a system appearance switch has to
        // repaint the cells (explicit colours are unaffected).
        screenChanged()
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        syncGrid()
        screenChanged()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        syncRenderer()
        syncGrid()
    }

    override func viewDidHide() {
        super.viewDidHide()
        syncRenderer()
    }

    override func viewDidUnhide() {
        super.viewDidUnhide()
        syncRenderer()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        screenChanged()
    }

    /// Recomputes the grid from the current bounds and tells the host.
    /// Forces a resize report even if the grid size did not change since the
    /// last attempt (which may have had no pane to address).
    func forceSyncGrid() {
        reported = (0, 0)
        syncGrid()
    }

    /// The grid a view of `bounds` can hold, or `nil` when it has no usable size yet.
    ///
    /// Pure so the rule is testable: a view that has not been laid out must never report a
    /// grid. Forwarding a degenerate size to the host resizes the terminal to 2×1 and throws
    /// its screen away, and an idle TUI never repaints — the pane would stay blank for good.
    static func gridSize(
        for bounds: CGSize,
        cellSize: CGSize = CoreTextRenderer.cellSize()
    ) -> (cols: UInt16, rows: UInt16)? {
        guard cellSize.width > 0, cellSize.height > 0 else { return nil }
        let columns = Int(bounds.width / cellSize.width)
        let lines = Int(bounds.height / cellSize.height)
        guard columns >= 2, lines >= 1,
              columns <= Int(UInt16.max), lines <= Int(UInt16.max)
        else { return nil }
        return (UInt16(columns), UInt16(lines))
    }

    func syncGrid() {
        // Only the *requested* size changes here. `cols`/`rows` describe the
        // cells we hold, and those must keep matching until the host answers
        // with a new snapshot — otherwise the renderer walks off the grid.
        guard let grid = Self.gridSize(for: bounds.size), grid != reported else { return }
        reported = grid
        onResize?(grid.cols, grid.rows)
    }

    func apply(snapshot: UnsafePointer<PgSnapshot>) {
        let view = snapshot.pointee
        cols = Int(view.cols)
        rows = Int(view.rows)
        cells = []
        cells.reserveCapacity(cols * rows)
        if let screen = view.screen {
            for row in 0..<rows {
                let cellsPointer = screen[row].cells
                if let cellsPointer {
                    for column in 0..<cols {
                        let cell = cellsPointer[column]
                        cells.append(
                            Cell(
                                codepoint: cell.codepoint,
                                attrs: UInt8(truncatingIfNeeded: cell.attrs),
                                fg: cell.fg,
                                bg: cell.bg,
                                width: cell.width
                            )
                        )
                    }
                } else {
                    cells.append(contentsOf: Array(repeating: Cell(
                        codepoint: 32, attrs: 0, fg: 0, bg: 0, width: 1
                    ), count: cols))
                }
            }
        }
        cursor = Cursor(
            x: Int(view.cursor.x),
            y: Int(view.cursor.y),
            visible: view.cursor.visible != 0
        )
        alt = view.alt_screen != 0
        reported = (UInt16(cols), UInt16(rows))
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let first = cells.prefix(40).map { cell -> Character in
                Unicode.Scalar(cell.codepoint).map(Character.init) ?? " "
            }
            // Blank-pane diagnosis: how many cells carry a glyph, and the last such row.
            let filled = cells.reduce(into: 0) { count, cell in
                if cell.codepoint != 0, cell.codepoint != 32 { count += 1 }
            }
            var lastRow = -1
            if cols > 0 {
                for row in stride(from: rows - 1, through: 0, by: -1) {
                    let slice = cells[row * cols ..< min((row + 1) * cols, cells.count)]
                    if slice.contains(where: { $0.codepoint != 0 && $0.codepoint != 32 }) {
                        lastRow = row
                        break
                    }
                }
            }
            FileHandle.standardError.write(
                Data("pigeon: grid pane=\(paneID ?? 0) \(cols)x\(rows) cursor=\(cursor.x),\(cursor.y) filled=\(filled) lastRow=\(lastRow) row0=\(String(first))\n".utf8)
            )
        }
        dirty = .everything
        screenChanged()
    }

    /// Patches the rows a `delta` frame carries. Returns `false` when the frame does not fit
    /// the grid this view holds (a resize raced it); the caller then re-attaches for a
    /// snapshot instead of drawing a torn screen.
    func apply(delta: UnsafePointer<PgDelta>) -> Bool {
        let frame = delta.pointee
        guard cols > 0, cells.count == cols * rows else { return false }
        if let changed = frame.rows {
            for index in 0..<Int(frame.row_count) {
                let row = changed[index]
                let y = Int(row.y)
                guard y < rows, Int(row.cols) == cols, let source = row.cells else { return false }
                dirty.mark(y)
                for column in 0..<cols {
                    let cell = source[column]
                    cells[y * cols + column] = Cell(
                        codepoint: cell.codepoint,
                        attrs: UInt8(truncatingIfNeeded: cell.attrs),
                        fg: cell.fg,
                        bg: cell.bg,
                        width: cell.width
                    )
                }
            }
        }
        if frame.has_cursor != 0 {
            cursor = Cursor(
                x: Int(frame.cursor.x),
                y: Int(frame.cursor.y),
                visible: frame.cursor.visible != 0
            )
        }
        if frame.alt_screen < 2 {
            alt = frame.alt_screen != 0
        }
        screenChanged()
        return true
    }

    /// Clicks belong to the container (divider drags, focus); NSView does not
    /// bubble unhandled mouse events, so forward them explicitly.
    override func mouseDown(with event: NSEvent) {
        nextResponder?.mouseDown(with: event)
    }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 116, page(1) { return }
        if event.keyCode == 121, page(-1) { return }
        guard let stroke = KeyStroke(event) else {
            super.keyDown(with: event)
            return
        }
        guard let bytes = KeyEncoder.encode(stroke) else { return }
        // Typing while scrolled back snaps to the live screen, like every terminal.
        onScrollToBottom?()
        onInput?(bytes)
    }
}

extension KeyStroke {
    /// Maps an AppKit event onto the pure [`KeyStroke`] model.
    init?(_ event: NSEvent) {
        let modifiers = event.modifierFlags
        let control = modifiers.contains(.control)
        let option = modifiers.contains(.option)
        let shift = modifiers.contains(.shift)
        let command = modifiers.contains(.command)

        let key: KeyStroke.Key
        switch event.keyCode {
        case 36, 76: key = .enter
        case 48: key = .tab
        case 51: key = .backspace
        case 53: key = .escape
        case 117: key = .delete
        case 123: key = .left
        case 124: key = .right
        case 125: key = .down
        case 126: key = .up
        case 115: key = .home
        case 119: key = .end
        case 116: key = .pageUp
        case 121: key = .pageDown
        case 122: key = .function(1)
        case 120: key = .function(2)
        case 99: key = .function(3)
        case 118: key = .function(4)
        case 96: key = .function(5)
        case 97: key = .function(6)
        case 98: key = .function(7)
        case 100: key = .function(8)
        case 101: key = .function(9)
        case 109: key = .function(10)
        case 103: key = .function(11)
        case 111: key = .function(12)
        default:
            guard let characters = event.charactersIgnoringModifiers,
                  let first = characters.first
            else { return nil }
            key = .character(first)
        }

        self.init(
            key: key,
            control: control,
            option: option,
            shift: shift,
            command: command
        )
    }
}
