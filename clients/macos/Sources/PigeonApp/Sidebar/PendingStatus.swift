import Foundation

/// A record offered for inspection, with its full acknowledgement destination.
struct PendingStatus: Equatable {
    var target: StatusAcknowledgement
    var status: SidebarSession.Status
    var agent: AgentInfo

    var canAcknowledge: Bool { status == .done || status == .error }
}
