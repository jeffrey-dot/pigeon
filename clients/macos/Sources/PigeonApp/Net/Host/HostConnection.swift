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
    /// The host sends every pane's `status` without an attach (`status_all`, protocol §5).
    var statusAll = false
    var heartbeat: Heartbeat?

    /// A new connection to the same host: screens stay on display and become stale (they
    /// re-attach with `since`), nothing is in flight any more, and the liveness clock restarts.
    mutating func reconnected(now: Date) {
        stale = seen
        requested = []
        heartbeat = Heartbeat(now: now)
    }

    /// `hello_ack` named a different host process: its `seq` numbers start over, so nothing
    /// drawn so far can be resumed.
    mutating func instanceChanged(to value: String) {
        if instance != value {
            stale = nil
            seq = [:]
        }
        instance = value
    }
}
