import Foundation

/// What the window knows about one session on one host: its panes, where it runs, and the
/// agent's last report. Built from `sessions` and `status` by `SessionReducer`.
struct SessionRow: Equatable {
    var id: Int
    var panes: [UInt32]
    var cwd: String
    var activePane: UInt32
    var app: String
    var title: String
    var status: SidebarSession.Status
    var seq: UInt32
    var unread: Int
    var lastSeen: Date
    /// Who owns the size when more than one window watches this session (protocol §6).
    var sizeSource: String = ""
    var clients: Int = 1
    /// Where the session lives; `local` for the host on this machine.
    var host: String = "local"
    /// The user's group, fixed when the session first appeared (`Grouping.pinnedGroup`):
    /// a `cd` later must not reshuffle the sidebar.
    var group: String = ""
    /// The agent's own report (name, task, since when), from the last `status`.
    var agent: AgentInfo = .none
}
