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
        CoreTextRenderer.draw(cells: cells, cols: cols, rows: rows, cursor: cursor, in: bounds)
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

    override func keyDown(with event: NSEvent) {
        guard let stroke = KeyStroke(event) else {
            super.keyDown(with: event)
            return
        }
        guard let bytes = KeyEncoder.encode(stroke) else { return }
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
