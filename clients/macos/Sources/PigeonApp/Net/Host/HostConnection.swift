import Foundation

/// Everything the window keeps for one machine (`docs/remote.md` §4: one connection per host,
/// N sessions on it). One value per host, so forgetting a host is one assignment, and no field
/// can be left behind when a connection is reset.
struct HostConnection<Row> {
    var rows: [Int: Row] = [:]
    var focusedPane: UInt32?
    var selectedSession: Int?
    /// Panes that have a screen on display.
    var seen: Set<UInt32> = []
    /// Panes whose picture was applied from this transport, not only cached before reconnect.
    var receivedScreen: Set<UInt32> = []
    /// Panes an `attach` was sent for and not yet answered.
    var requested: Set<UInt32> = []
    /// Snapshots that arrived before their pane view existed.
    var pending: [UInt32: Data] = [:]
    /// Last frame `seq` drawn per pane: a `delta` must be exactly the next one, otherwise a
    /// frame went missing and the pane re-attaches for a snapshot (protocol §3.3).
    var seq: [UInt32: UInt32] = [:]
    /// Panes still showing what a dropped connection drew: their next `attach` carries
    /// `since` so the host replays only the missed deltas (issue #4).
    var stale: Set<UInt32>?
    /// `hello_ack.instance`; `since` is only meaningful to that same process.
    var instance: String?
    /// Identifies callbacks already queued on the main thread by a replaced transport.
    private(set) var generation = UUID()
    private(set) var ready = false
    /// The host sends every pane's `status` without an attach (`status_all`, protocol §5).
    var statusAll = false
    var statusSnapshot = false
    var heartbeat: Heartbeat?

    /// A new connection to the same host: screens stay on display and become stale (they
    /// re-attach with `since`), nothing is in flight any more, and the liveness clock restarts.
    mutating func reconnected(now: Date) {
        generation = UUID()
        ready = false
        statusAll = false
        statusSnapshot = false
        receivedScreen = []
        stale = seen
        requested = []
        pending = [:]
        heartbeat = Heartbeat(now: now)
    }

    /// `hello_ack` named a different host process: its `seq` numbers start over, so nothing
    /// drawn so far can be resumed.
    mutating func instanceChanged(to value: String) {
        // Older hosts without an instance cannot safely reuse durable event identities.
        let value = value.isEmpty ? UUID().uuidString : value
        if instance != value {
            stale = nil
            seq = [:]
            rows = [:]
            seen = []
            receivedScreen = []
            requested = []
            pending = [:]
            focusedPane = nil
            selectedSession = nil
        }
        instance = value
        ready = true
    }

    func accepts(generation: UUID) -> Bool { self.generation == generation }

    func accepts(_ acknowledgement: StatusAcknowledgement, host: String) -> Bool {
        ready && acknowledgement.matches(host: host, instance: instance)
    }

    mutating func disconnected() {
        ready = false
        statusAll = false
        statusSnapshot = false
        receivedScreen = []
    }
}
