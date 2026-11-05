import Foundation

/// A cancelled press stays cancelled even if a later frame restores the same target.
struct TerminalLinkPress {
    private(set) var url: URL?
    let point: TerminalSelection.Point

    init(url: URL, point: TerminalSelection.Point) {
        self.url = url
        self.point = point
    }

    mutating func cancel() { url = nil }

    mutating func validate(cells: [TerminalCell], columns: Int, rows: Int) {
        guard let url else { return }
        guard point.row >= 0, point.row < rows, point.column >= 0, point.column < columns,
              cells.indices.contains(point.row * columns + point.column),
              TerminalLinks.openingURL(cells[point.row * columns + point.column].hyperlink) == url
        else { cancel(); return }
    }
}
