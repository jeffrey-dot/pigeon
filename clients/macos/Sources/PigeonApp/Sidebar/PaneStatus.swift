import Foundation

/// One pane's latest report and terminal events not yet seen by this client.
/// Screen seq and status seq have different lifetimes; this only tracks status.
struct PaneStatus: Equatable {
    var status: SidebarSession.Status
    var seq: UInt32
    var agent: AgentInfo
    var newestFinished: UInt32 = 0
    var unread: Set<UInt32> = []
    /// Suppresses repeat acks while snapshots/layouts keep arriving for the same report.
    var acknowledgedSeq: UInt32?
}
