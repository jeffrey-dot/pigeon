import AppKit
import PigeonCore

/// The single terminal pane: shows a grid snapshot, sends keys, reports resizes.
final class PaneView: NSView {
    struct Cell {
        var codepoint: UInt32
        var attrs: UInt8
        var fg: UInt32
        var bg: UInt32
        var width: UInt8

        static let blank = Cell(codepoint: 32, attrs: 0, fg: 0, bg: 0, width: 1)
    }

    struct Cursor {
        var x: Int
        var y: Int
        var visible: Bool
    }

    /// Set by the container once the pane view is created.
    var paneID: UInt32?
    private(set) var cols = 80
    private(set) var rows = 24
    private(set) var cells: [Cell] = []
    /// Scrollback from the last snapshot, oldest first. Empty on the alternate screen.
    private(set) var history: [[Cell]] = []
    /// Lines scrolled up from the live screen; `0` is the live grid.
    private(set) var displayOffset = 0
    /// The alternate screen keeps no scrollback: vim/htop/less own the whole screen.
    private(set) var alt = false
    /// Lines the user asked for while the host was fetching a fresh history.
    private var pendingScroll: Int?
    /// The screen frozen when scrolling started, so streaming output cannot tear the
    /// history/screen seam the user is reading.
    private(set) var frozenScreen: [Cell]?
    /// Asks the owner to re-attach, which is how a fresh history arrives (protocol §3.3).
    var onNeedHistory: ((UInt32) -> Void)?
    private(set) var cursor = Cursor(x: 0, y: 0, visible: false)

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
        CoreTextRenderer.draw(
            cells: displayOffset > 0 ? scrolledCells() : cells,
            cols: cols,
            rows: rows,
            // Scrolled back: the cursor is not on screen, so drawing it would lie.
            cursor: displayOffset > 0 ? Cursor(x: 0, y: 0, visible: false) : cursor,
            in: bounds
        )
        drawScrollIndicator()
    }

    /// The grid as it looks scrolled back: `displayOffset` history lines on top, the rest
    /// of the screen underneath.
    func scrolledCells() -> [Cell] {
        guard cols > 0, displayOffset > 0, !history.isEmpty else { return cells }
        let offset = Self.clampedOffset(displayOffset, historyLines: history.count)
        var visible: [Cell] = []
        visible.reserveCapacity(cols * rows)
        for row in history.suffix(offset) {
            visible.append(contentsOf: row.prefix(cols))
            if row.count < cols {
                visible.append(contentsOf: Array(repeating: .blank, count: cols - row.count))
            }
        }
        visible.append(contentsOf: (frozenScreen ?? cells).prefix(max(0, rows - offset) * cols))
        while visible.count < cols * rows {
            visible.append(.blank)
        }
        return visible
    }

    /// Clamps a scroll position to the history the pane actually holds.
    static func clampedOffset(_ offset: Int, historyLines: Int) -> Int {
        min(max(offset, 0), max(historyLines, 0))
    }

    static func canScroll(alt: Bool, historyLines: Int) -> Bool {
        !alt && historyLines > 0
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

    override func scrollWheel(with event: NSEvent) {
        guard Self.canScroll(alt: alt, historyLines: history.count) else { return }
        let (lines, remainder) = Self.wheelLines(
            deltaY: event.scrollingDeltaY,
            precise: event.hasPreciseScrollingDeltas,
            cellHeight: CoreTextRenderer.cellSize().height,
            remainder: wheelRemainder
        )
        wheelRemainder = remainder
        guard lines != 0 else { return }
        scroll(lines: lines)
    }

    /// Positive `lines` moves back in time, matching an upward wheel.
    ///
    /// Scrolling back the first time asks for a fresh history: the cached one ends where the
    /// screen was when it arrived, so scrolling over it would show a gap.
    func scroll(lines: Int) {
        guard lines != 0 else { return }
        if lines > 0, frozenScreen == nil, let paneID {
            pendingScroll = (pendingScroll ?? 0) + lines
            onNeedHistory?(paneID)
            return
        }
        applyScroll(lines)
    }

    private func applyScroll(_ lines: Int) {
        let offset = Self.clampedOffset(displayOffset + lines, historyLines: history.count)
        if offset == 0 {
            frozenScreen = nil
        }
        guard offset != displayOffset else { return }
        displayOffset = offset
        needsDisplay = true
    }

    func scrollToBottom() {
        pendingScroll = nil
        frozenScreen = nil
        guard displayOffset != 0 else { return }
        displayOffset = 0
        needsDisplay = true
    }

    /// A thumb on the right edge: "you are not at the bottom" has to be visible, or the
    /// user cannot tell scrolled-back output from output that stopped arriving.
    private func drawScrollIndicator() {
        let total = rows + history.count
        guard history.count > 0, total > 0, bounds.height > 0, bounds.width > 8 else { return }
        let width: CGFloat = 3
        let height = max(bounds.height * CGFloat(rows) / CGFloat(total), 18)
        let travel = bounds.height - height
        let progress = CGFloat(displayOffset) / CGFloat(history.count)
        let rect = NSRect(
            x: bounds.maxX - width - 2,
            y: travel * progress,
            width: width,
            height: height
        )
        NSColor.secondaryLabelColor.withAlphaComponent(0.45).setFill()
        NSBezierPath(roundedRect: rect, xRadius: width / 2, yRadius: width / 2).fill()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        // The grid's default fg/bg come from the theme, so a system appearance switch has to
        // repaint the cells (explicit colours are unaffected).
        needsDisplay = true
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        syncGrid()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        syncGrid()
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
        history = grid(view.history, count: Int(view.history_count))
        // Reconnecting, resizing and the alt screen all change what the stored offset means.
        if alt {
            displayOffset = 0
            pendingScroll = nil
            frozenScreen = nil
        } else if let pending = pendingScroll {
            // The history the user asked for just arrived: freeze this screen+history pair.
            pendingScroll = nil
            displayOffset = Self.clampedOffset(pending, historyLines: history.count)
            frozenScreen = displayOffset > 0 ? cells : nil
        } else {
            displayOffset = Self.clampedOffset(displayOffset, historyLines: history.count)
            if displayOffset == 0 {
                frozenScreen = nil
            }
        }
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
        needsDisplay = true
    }

    /// Clicks belong to the container (divider drags, focus); NSView does not
    /// bubble unhandled mouse events, so forward them explicitly.
    override func mouseDown(with event: NSEvent) {
        nextResponder?.mouseDown(with: event)
    }

    /// Reads a snapshot's row array (`screen`/`history`) into plain cells.
    private func grid(_ rowsPointer: UnsafePointer<PgRow>?, count: Int) -> [[Cell]] {
        guard let rowsPointer, count > 0, cols > 0 else { return [] }
        return (0..<count).map { index in
            guard let cellsPointer = rowsPointer[index].cells else {
                return Array(repeating: .blank, count: cols)
            }
            return (0..<cols).map { column in
                let cell = cellsPointer[column]
                return Cell(
                    codepoint: cell.codepoint,
                    attrs: UInt8(truncatingIfNeeded: cell.attrs),
                    fg: cell.fg,
                    bg: cell.bg,
                    width: cell.width
                )
            }
        }
    }

    override func keyDown(with event: NSEvent) {
        // Page keys page the local history on the live screen; on the alternate screen the
        // program (`less`, `vim`) wants them, so they keep going to the pane.
        let page = max(rows - 1, 1)
        if !alt, !history.isEmpty {
            if event.keyCode == 116 {
                scroll(lines: page)
                return
            }
            if event.keyCode == 121 {
                scroll(lines: -page)
                return
            }
        }
        guard let stroke = KeyStroke(event) else {
            super.keyDown(with: event)
            return
        }
        guard let bytes = KeyEncoder.encode(stroke) else { return }
        // Typing while scrolled back snaps to the live screen, like every terminal.
        scrollToBottom()
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
