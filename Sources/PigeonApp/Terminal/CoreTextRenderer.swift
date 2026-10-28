import AppKit
import CoreText
import PigeonCore

/// Draws a cell grid with CoreText. The only file allowed to import CoreText.
enum CoreTextRenderer {
    static let fontSize: CGFloat = 13

    /// A run of cells sharing one colour, pinned to the column it starts at.
    ///
    /// Cells are positioned by column, never by concatenating a whole row: a CJK
    /// glyph's advance is ~1.6 monospace cells, so letting CoreText flow the line
    /// would shift everything after a wide character to the left.
    struct TextRun: Equatable {
        let startColumn: Int
        let text: String
        let colorValue: UInt32
        /// A wide glyph gets a run of its own: CoreText would otherwise advance by
        /// its own (~1.6 cell) metric instead of the two columns it owns.
        let wide: Bool
    }

    static func font(bold: Bool) -> NSFont {
        let base = NSFont.monospacedSystemFont(ofSize: fontSize, weight: bold ? .bold : .regular)
        return base
    }

    static func cellSize() -> CGSize {
        let font = font(bold: false)
        let advance = ("M" as NSString).size(withAttributes: [.font: font])
        let line = font.ascender - font.descender + font.leading
        return CGSize(width: max(advance.width, 1), height: max(line, 1))
    }

    static func draw(
        cells: [PaneView.Cell],
        cols: Int,
        rows: Int,
        cursor: PaneView.Cursor,
        in bounds: CGRect
    ) {
        // The terminal's own background, painted even before the first snapshot: a pane
        // with no cells must still look like a terminal, not like an empty window (which
        // turns white under a light app appearance).
        NSColor(calibratedRed: 0.11, green: 0.11, blue: 0.13, alpha: 1).setFill()
        bounds.fill()
        // A resize can land between the request and the new snapshot; draw only
        // what we hold instead of trapping on a half-updated grid.
        guard cols > 0, rows > 0, !cells.isEmpty, cells.count >= cols * rows else { return }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: draw cells=\(cells.count) size=\(bounds.size)\n".utf8))
        }
        let size = cellSize()
        let font = font(bold: false)

        for row in 0..<rows {
            let originY = bounds.height - CGFloat(row + 1) * size.height
            let start = row * cols
            let end = min(start + cols, cells.count)
            guard start < end else { continue }
            let rowCells = Array(cells[start..<end])
            guard rowCells.count == cols else { continue }

            // Backgrounds first, then the glyph run on top.
            for (column, cell) in rowCells.enumerated() {
                let (fg, bg) = resolved(cell)
                guard bg != defaultBackground else { continue }
                color(bg).setFill()
                CGRect(
                    x: CGFloat(column) * size.width,
                    y: originY,
                    width: size.width,
                    height: size.height
                ).fill()
                _ = fg
            }

            guard let context = NSGraphicsContext.current?.cgContext else { continue }
            context.textMatrix = .identity
            for run in Self.textRuns(for: rowCells) {
                let attributed = NSAttributedString(
                    string: run.text,
                    attributes: [
                        NSAttributedString.Key(kCTFontAttributeName as String): font,
                        NSAttributedString.Key(kCTForegroundColorAttributeName as String):
                            color(run.colorValue).cgColor,
                    ]
                )
                let line = CTLineCreateWithAttributedString(attributed)
                context.textPosition = CGPoint(
                    x: CGFloat(run.startColumn) * size.width,
                    y: originY + (-font.descender)
                )
                CTLineDraw(line, context)
            }
        }

        if cursor.visible, cursor.x < cols, cursor.y < rows {
            NSColor(calibratedWhite: 0.9, alpha: 0.35).setFill()
            CGRect(
                x: CGFloat(cursor.x) * size.width,
                y: bounds.height - CGFloat(cursor.y + 1) * size.height,
                width: size.width,
                height: size.height
            ).fill()
        }
    }

    /// Groups a row into runs, skipping continuation cells (width 0) so the cell
    /// after a wide character keeps its own column.
    static func textRuns(for rowCells: [PaneView.Cell]) -> [TextRun] {
        var runs: [TextRun] = []
        for (column, cell) in rowCells.enumerated() {
            guard cell.width != 0, let scalar = Unicode.Scalar(cell.codepoint) else { continue }
            let (fg, _) = resolved(cell)
            let character = String(Character(scalar))
            let wide = cell.width == 2
            if let last = runs.last, !wide, !last.wide, last.colorValue == fg {
                runs[runs.count - 1] = TextRun(
                    startColumn: last.startColumn,
                    text: last.text + character,
                    colorValue: fg,
                    wide: false
                )
            } else {
                runs.append(
                    TextRun(startColumn: column, text: character, colorValue: fg, wide: wide)
                )
            }
        }
        return runs
    }

    // MARK: - colours

    private static let defaultForeground: UInt32 = 0x00e5_e5e5
    private static let defaultBackground: UInt32 = 0x0000_0000

    private static func resolved(_ cell: PaneView.Cell) -> (UInt32, UInt32) {
        var fg = cell.fg == 0 ? defaultForeground : cell.fg
        var bg = cell.bg == 0 ? defaultBackground : cell.bg
        if cell.attrs & 0x08 != 0 { swap(&fg, &bg) } // reverse video
        return (fg, bg)
    }

    private static func color(_ value: UInt32) -> NSColor {
        if value & 0xff00_0000 == 0x0100_0000 {
            return palette(Int(value & 0xff))
        }
        return NSColor(
            calibratedRed: CGFloat((value >> 16) & 0xff) / 255,
            green: CGFloat((value >> 8) & 0xff) / 255,
            blue: CGFloat(value & 0xff) / 255,
            alpha: 1
        )
    }

    /// xterm's 256-colour table.
    private static func palette(_ index: Int) -> NSColor {
        let ansi: [(UInt32, UInt32, UInt32)] = [
            (0, 0, 0), (205, 0, 0), (0, 205, 0), (205, 205, 0),
            (0, 0, 238), (205, 0, 205), (0, 205, 205), (229, 229, 229),
            (127, 127, 127), (255, 0, 0), (0, 255, 0), (255, 255, 0),
            (92, 92, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
        ]
        if index < 16 {
            let (r, g, b) = ansi[index]
            return NSColor(
                calibratedRed: CGFloat(r) / 255,
                green: CGFloat(g) / 255,
                blue: CGFloat(b) / 255,
                alpha: 1
            )
        }
        if index < 232 {
            let value = index - 16
            let steps: [CGFloat] = [0, 95, 135, 175, 215, 255]
            let r = steps[(value / 36) % 6] / 255
            let g = steps[(value / 6) % 6] / 255
            let b = steps[value % 6] / 255
            return NSColor(calibratedRed: r, green: g, blue: b, alpha: 1)
        }
        let level = CGFloat(8 + (index - 232) * 10) / 255
        return NSColor(calibratedRed: level, green: level, blue: level, alpha: 1)
    }
}
