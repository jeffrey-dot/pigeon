import Foundation

/// The rules for folding host messages into the rows of one host, as pure functions: the
/// state goes in, the new state and the side effects to perform come out. `AppDelegate`
/// decodes the wire message, calls these, and carries out the effects (send, save, redraw).
enum SessionReducer {
    /// One session as the `sessions` list reports it, already decoded from the wire.
    struct Listed: Equatable {
        var id: Int
        var panes: [UInt32]
        var cwd: String
        /// 0 when the host did not say.
        var activePane: UInt32
        /// The program name, or the window title when that is all there is; empty = unknown.
        var program: String
    }

    /// A `status` message, decoded.
    struct Report: Equatable {
        var pane: UInt32
        /// nil for a state name this build does not know (the row keeps its status).
        var status: SidebarSession.Status?
        var seq: UInt32
        var agent: AgentInfo
    }

    enum Effect: Equatable {
        /// Tell the host the user has seen this record (`ack_status`).
        case ack(pane: UInt32, seq: UInt32)
        /// The persisted unread count for this session changed.
        case saveUnread(SessionRef, Int)
    }

    /// Applies a `status`. `onScreen`: the user is looking at this host's selected session.
    /// Only `done`/`error` count as unread or get acked (`docs/ui.md` §5); `working` and
    /// `blocked` are the agent's live state and stay until the host clears them.
    static func apply(
        _ report: Report, to rows: inout [Int: SessionRow], host: String, selected: Int?, onScreen: Bool
    ) -> [Effect] {
        guard let id = rows.first(where: { $0.value.panes.contains(report.pane) })?.key,
              var row = rows[id]
        else { return [] }
        if let status = report.status { row.status = status }
        row.seq = report.seq
        row.agent = report.agent
        row.lastSeen = report.agent.since
        let finished = row.status == .done || row.status == .error
        let seen = onScreen && selected == id
        var effects: [Effect] = []
        if finished, !seen {
            row.unread += 1
            effects.append(.saveUnread(SessionRef(host: host, session: id), row.unread))
        }
        rows[id] = row
        if finished, seen { effects.append(.ack(pane: row.activePane, seq: row.seq)) }
        return effects
    }

    /// Replaces the rows with a fresh `sessions` list, keeping what only the window knows
    /// (status, unread, the pinned group, size owner) for sessions that were already there.
    /// `unread` reads the persisted count for a session seen for the first time this run;
    /// `gitRoot` finds a new session's project root (nil: group by the cwd's last component).
    static func merge(
        _ listed: [Listed], into previous: [Int: SessionRow], host: String, now: Date,
        unread: (SessionRef) -> Int, gitRoot: (String) -> String?
    ) -> [Int: SessionRow] {
        var updated: [Int: SessionRow] = [:]
        for item in listed {
            guard let first = item.panes.first else { continue }
            let old = previous[item.id]
            // No focused pane: the host has nothing to say, keep the last name. Otherwise the
            // program (or the title the host sent in its place), as the host reported it.
            let app = item.activePane == 0 ? (old?.app ?? "shell") : item.program
            updated[item.id] = SessionRow(
                id: item.id,
                panes: item.panes,
                cwd: item.cwd,
                activePane: item.activePane != 0 ? item.activePane : (old?.activePane ?? first),
                app: app,
                title: "session \(item.id)",
                status: old?.status ?? .idle,
                seq: old?.seq ?? 0,
                unread: old?.unread ?? unread(SessionRef(host: host, session: item.id)),
                lastSeen: old?.lastSeen ?? now,
                sizeSource: old?.sizeSource ?? "",
                clients: old?.clients ?? 1,
                host: host,
                // Only a new session asks for its git root: the group is pinned after that.
                group: old.map(\.group).flatMap { $0.isEmpty ? nil : $0 }
                    ?? Grouping.pinnedGroup(existing: nil, cwd: item.cwd, gitRoot: gitRoot(item.cwd)),
                agent: old?.agent ?? .none)
        }
        return updated
    }
}
