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
        _ report: Report, to rows: inout [Int: SessionRow], host: String, selected: Int?, onScreen: Bool,
        visiblePanes: Set<UInt32>? = nil
    ) -> [Effect] {
        guard let id = rows.first(where: { $0.value.panes.contains(report.pane) })?.key,
              var row = rows[id]
        else { return [] }
        guard report.status != nil else { return [] }
        var pane = row.paneStatuses[report.pane] ?? PaneStatus(
            status: row.panes.count == 1 ? row.status : .idle, seq: 0, agent: .none)
        if pane.seq != report.seq { pane.acknowledgedSeq = nil }
        if let status = report.status { pane.status = status }
        pane.seq = report.seq
        pane.agent = report.agent
        let finished = report.status == .done || report.status == .error
        // A replay can mean the last ack never reached the host. Re-ack a displayed
        // report, but markSeen still suppresses repeats caused only by UI refreshes.
        if finished { pane.acknowledgedSeq = nil }
        let seen = onScreen && selected == id && (visiblePanes?.contains(report.pane) ?? true)
        var effects: [Effect] = []
        if finished, !seen, report.seq > pane.newestFinished {
            pane.unread.insert(report.seq)
            row.unread += 1
            effects.append(.saveUnread(SessionRef(host: host, session: id), row.unread))
        }
        if finished { pane.newestFinished = max(pane.newestFinished, report.seq) }
        row.paneStatuses[report.pane] = pane
        aggregate(&row)
        if finished, seen {
            effects += acknowledge(panes: [report.pane], row: &row, host: host)
        }
        rows[id] = row
        return effects
    }

    /// Visible panes are supplied by the view; hidden siblings retain their unread events.
    static func markSeen(
        _ panes: Set<UInt32>, in session: Int, rows: inout [Int: SessionRow], host: String
    ) -> [Effect] {
        guard var row = rows[session] else { return [] }
        let effects = acknowledge(panes: panes, row: &row, host: host)
        rows[session] = row
        return effects
    }

    private static func acknowledge(panes: Set<UInt32>, row: inout SessionRow, host: String) -> [Effect] {
        var effects: [Effect] = []
        let previousUnread = row.unread
        for id in panes.sorted() {
            guard var pane = row.paneStatuses[id] else { continue }
            var sequences = pane.unread
            if pane.status == .done || pane.status == .error,
               pane.seq > 0, pane.acknowledgedSeq != pane.seq {
                sequences.insert(pane.seq)
            }
            effects += sequences.sorted().map { .ack(pane: id, seq: $0) }
            row.unread = max(0, row.unread - pane.unread.count)
            pane.unread = []
            if sequences.contains(pane.seq) { pane.acknowledgedSeq = pane.seq }
            row.paneStatuses[id] = pane
        }
        // Counts written by an older client have no pane identity. Clear that remainder
        // only after every pane is displayed, never when a zoom hides its siblings.
        if Set(row.panes).isSubset(of: panes) { row.unread = 0 }
        if row.unread != previousUnread {
            effects.append(.saveUnread(SessionRef(host: host, session: row.id), row.unread))
        }
        return effects
    }

    private static func aggregate(_ row: inout SessionRow) {
        let winner = row.paneStatuses.min { left, right in
            if left.value.status != right.value.status { return left.value.status < right.value.status }
            return left.key < right.key
        }
        guard let (pane, report) = winner else {
            row.statusPane = nil
            row.status = .idle
            row.seq = 0
            row.agent = .none
            return
        }
        row.statusPane = pane
        row.status = report.status
        row.seq = report.seq
        row.agent = report.agent
        row.lastSeen = report.agent.since
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
                agent: old?.agent ?? .none,
                paneStatuses: old?.paneStatuses.filter { item.panes.contains($0.key) } ?? [:],
                statusPane: old?.statusPane)
            if var row = updated[item.id], let old, !old.paneStatuses.isEmpty {
                let removedUnread = old.paneStatuses.filter { !item.panes.contains($0.key) }
                    .values.reduce(0) { $0 + $1.unread.count }
                row.unread = max(0, row.unread - removedUnread)
                aggregate(&row)
                updated[item.id] = row
            }
        }
        return updated
    }
}
