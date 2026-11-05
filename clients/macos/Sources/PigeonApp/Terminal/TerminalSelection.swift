/// Pointer gesture only. Core owns word/line expansion, wide cells and copied text.
struct TerminalSelection: Equatable {
    struct Point: Equatable, Comparable {
        var row: Int
        var column: Int

        static func < (lhs: Point, rhs: Point) -> Bool {
            lhs.row != rhs.row ? lhs.row < rhs.row : lhs.column < rhs.column
        }
    }

    /// How a drag extends: by cell, by word (double-click) or by line (triple-click).
    enum Unit: Equatable {
        case cell
        case word
        case line
    }

    var anchor: Point
    var head: Point
    var unit: Unit
    var rectangle: Bool

    init(at point: Point, unit: Unit = .cell, rectangle: Bool = false) {
        anchor = point
        head = point
        self.unit = unit
        self.rectangle = rectangle
    }
}
