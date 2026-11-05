/// One row's worth of a selection: columns `first...last` of `row`.
struct SelectionSpan: Equatable, Sendable {
    var row: Int
    var first: Int
    var last: Int
}
