import AppKit
import CoreText
import PigeonCore

/// Draws a cell grid with CoreText. The only file allowed to import CoreText.
enum CoreTextRenderer {
    static let fontSize: CGFloat = 13
    /// harp's terminal line-height ratio (`DEFAULT_UI_LINE_HEIGHT_RATIO` in
    /// `crates/warpui_core/src/elements/gui/text.rs:33`, used as the terminal default).
    static let lineHeightRatio: CGFloat = 1.2
    /// harp ships "Hack" (`app/src/settings/font.rs:11`); when it is not installed we keep the
    /// system monospace rather than bundling a font file.
    static let fontName = "Hack"

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

    static func draw(
        cells: [PaneView.Cell],
        cols: Int,
        rows: Int,
        cursor: PaneView.Cursor,
        in bounds: CGRect
    ) {
        // The terminal's theme background (`docs/ui.md` §7.1), painted even before the first
        // snapshot so an empty pane still looks like a terminal.
        SidebarTheme.terminalBackground.setFill()
        bounds.fill()
        // A resize can land between the request and the new snapshot; draw only
        // what we hold instead of trapping on a half-updated grid.
        guard cols > 0, rows > 0, !cells.isEmpty, cells.count >= cols * rows else { return }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: draw cells=\(cells.count) size=\(bounds.size)\n".utf8))
        }
        let size = cellSize()
        let font = font(bold: false)
        let defaults = defaultColors(for: NSAppearance.currentDrawing())

        for row in 0..<rows {
            let originY = bounds.height - CGFloat(row + 1) * size.height
            let start = row * cols
            let end = min(start + cols, cells.count)
            guard start < end else { continue }
            let rowCells = Array(cells[start..<end])
            guard rowCells.count == cols else { continue }

            // Backgrounds first, then the glyph run on top.
            for (column, cell) in rowCells.enumerated() {
                let (fg, bg) = resolved(cell, defaults: defaults)
                guard bg != defaults.bg else { continue }
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
            for run in Self.textRuns(for: rowCells, defaults: defaults) {
                let attributed = NSAttributedString(
                    string: run.text,
                    attributes: [
                        NSAttributedString.Key(kCTFontAttributeName as String): font,
                        NSAttributedString.Key(kCTForegroundColorAttributeName as String):
                            textColor(run.colorValue).cgColor,
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
            SidebarTheme.terminalForeground.withAlphaComponent(0.35).setFill()
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
    static func textRuns(
        for rowCells: [PaneView.Cell],
        defaults: (fg: UInt32, bg: UInt32) = (0x00e5e5e5, 0x000000)
    ) -> [TextRun] {
        var runs: [TextRun] = []
        for (column, cell) in rowCells.enumerated() {
            guard cell.width != 0, let scalar = Unicode.Scalar(cell.codepoint) else { continue }
            let (fg, _) = resolved(cell, defaults: defaults)
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

    /// The colours a cell with no explicit colour inherits, resolved for the appearance that
    /// is drawing: harp's `theme.foreground()` / `theme.background()` (`docs/ui.md` §7.1).
    static func defaultColors(for appearance: NSAppearance) -> (fg: UInt32, bg: UInt32) {
        var colors: (fg: UInt32, bg: UInt32) = (0, 0)
        appearance.performAsCurrentDrawingAppearance {
            colors = (rgb(SidebarTheme.terminalForeground), rgb(SidebarTheme.terminalBackground))
        }
        return colors
    }

    /// `0x00RRGGBB` in the form `color(_:)` understands.
    private static func rgb(_ color: NSColor) -> UInt32 {
        let srgb = color.usingColorSpace(.sRGB) ?? color
        let red = UInt32((srgb.redComponent * 255).rounded())
        let green = UInt32((srgb.greenComponent * 255).rounded())
        let blue = UInt32((srgb.blueComponent * 255).rounded())
        return (red << 16) | (green << 8) | blue
    }

    /// A cell's own colours stay untouched; only the "no colour" sentinel (0) uses the theme.
    private static func resolved(
        _ cell: PaneView.Cell,
        defaults: (fg: UInt32, bg: UInt32)
    ) -> (UInt32, UInt32) {
        var fg = cell.fg == 0 ? defaults.fg : cell.fg
        var bg = cell.bg == 0 ? defaults.bg : cell.bg
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

    /// Cell text goes through the contrast floor: a TUI's greys are chosen for a dark canvas
    /// and vanish on the light card (see `TerminalPalette.readable`).
    private static func textColor(_ value: UInt32) -> NSColor {
        TerminalPalette.readable(color(value), on: SidebarTheme.terminalBackground)
    }

    /// ANSI 0–15 come from harp's palette (per appearance); 16+ keep xterm's 256-colour cube.
    private static func palette(_ index: Int) -> NSColor {
        if let ansi = TerminalPalette.color(index, appearance: NSAppearance.currentDrawing()) {
            return ansi
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
