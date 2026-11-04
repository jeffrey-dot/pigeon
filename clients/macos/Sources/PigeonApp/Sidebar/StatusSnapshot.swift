import Foundation

/// Owned values copied from the core's validated status_snapshot (protocol §3.4).
struct StatusSnapshot: Equatable {
    var pane: UInt32
    var revision: UInt64
    var records: [Record]

    struct Record: Equatable {
        var identity: StatusRecord
        var status: SidebarSession.Status
        var agent: AgentInfo

        var finished: Bool { status == .done || status == .error }
    }
}
