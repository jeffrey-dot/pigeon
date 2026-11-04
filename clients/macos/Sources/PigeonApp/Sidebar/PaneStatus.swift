import Foundation

/// One pane's latest report and terminal events not yet seen by this client.
/// Screen seq and status seq have different lifetimes; this only tracks status.
struct PaneStatus: Equatable {
    var status: SidebarSession.Status
    var seq: UInt32
    var agent: AgentInfo
    var record: String = ""
    var receipt = Receipt()
    /// Suppresses repeat acks while snapshots/layouts keep arriving for the same report.
    var acknowledged: StatusRecord?
    /// Orders current reports only. It must never be used to discard unseen terminal records.
    var newestSeq: UInt32 = 0
    var current = true

    var identity: StatusRecord { StatusRecord(seq: seq, record: record) }

    /// Only event identities survive a launch. Cached live reports are not evidence that
    /// the user has viewed a report from the new connection.
    struct Receipt: Codable, Equatable {
        var unread: Set<StatusRecord> = []
        var seen: Set<StatusRecord> = []
    }
}
