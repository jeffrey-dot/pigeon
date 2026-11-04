import Foundation

/// A panel selection is bound to the transport that supplied its text, not only a pane id.
struct StatusInspection {
    var session: SessionRef
    var instance: String
    var generation: UUID
    private(set) var records: [PendingStatus] = []
    private var selection: StatusAcknowledgement?

    init(session: SessionRef, instance: String, generation: UUID) {
        self.session = session
        self.instance = instance
        self.generation = generation
    }

    var selected: PendingStatus? { records.first { $0.target == selection } }

    mutating func update(_ records: [PendingStatus]) {
        self.records = records
        // A disappearing record must not silently arm the button for its successor.
        if !records.contains(where: { $0.target == selection }) { selection = nil }
    }

    mutating func select(_ target: StatusAcknowledgement) {
        selection = records.first { $0.target == target }?.target
    }

    mutating func clearSelection() { selection = nil }

    func matches(host: String, session: Int?, instance: String?, generation: UUID) -> Bool {
        self.session.host == host && self.session.session == session
            && self.instance == instance && self.generation == generation
    }
}
