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
        var record: String = ""
    }

    enum Effect: Equatable {
        case ack(StatusAcknowledgement)
        case saveStatus(SessionRef, instance: String, receipts: [UInt32: PaneStatus.Receipt])
    }

    /// Visibility is explicit: selection alone is not evidence of a displayed pane.
    static func apply(
        _ report: Report, to rows: inout [Int: SessionRow], host: String, instance: String,
        selected: Int?, visiblePanes: Set<UInt32>
    ) -> [Effect] {
        guard let status = report.status,
              let id = rows.first(where: { $0.value.panes.contains(report.pane) })?.key,
              var row = rows[id], row.host == host, row.instance == instance
        else { return [] }
        var pane = row.paneStatuses[report.pane] ?? PaneStatus(status: .idle, seq: 0, agent: .none)
        let before = pane.receipt
        let identity = StatusRecord(seq: report.seq, record: report.record)
        let finished = status == .done || status == .error
        if finished, report.seq > 0, !pane.receipt.seen.contains(identity) {
            pane.receipt.unread.insert(identity)
        }
        // The host may expose a lower seq after the displayed record is acked/cleared.
        // While a newer report remains current, old arrivals cannot roll it back, but
        // every unseen terminal identity above is retained independently of this choice.
        let alreadySeen = pane.receipt.seen.contains(identity)
        let replace = !alreadySeen && (!pane.current || report.seq == 0 || report.seq >= pane.newestSeq
            || pane.seq == 0 || pane.acknowledged != nil)
        if replace {
            pane.status = status
            pane.seq = report.seq
            pane.record = report.record
            pane.agent = report.agent
            pane.acknowledged = nil
            pane.current = true
        }
        pane.newestSeq = max(pane.newestSeq, report.seq)
        row.paneStatuses[report.pane] = pane
        aggregate(&row)
        var effects: [Effect] = []
        if finished, report.seq > 0, alreadySeen {
            // This exact record was already viewed, even if the current pane is hidden.
            effects = [.ack(StatusAcknowledgement(host: host, instance: instance,
                                                  pane: report.pane, record: identity))]
        } else if replace, selected == id, visiblePanes.contains(report.pane) {
            effects = acknowledge(panes: [report.pane], row: &row)
        }
        updateUnread(&row)
        if before != row.paneStatuses[report.pane]?.receipt { effects.append(saveEffect(row)) }
        rows[id] = row
        return effects
    }

    /// Only each visible pane's current terminal report has been presented. Older pending
    /// identities stay unread until the host presents them again; a newer seq is not an ack.
    static func markSeen(
        _ panes: Set<UInt32>, in session: Int, rows: inout [Int: SessionRow], host: String, instance: String
    ) -> [Effect] {
        guard var row = rows[session], row.host == host, row.instance == instance else { return [] }
        let before = row.paneStatuses.mapValues(\.receipt)
        var effects = acknowledge(panes: panes, row: &row)
        updateUnread(&row)
        if before != row.paneStatuses.mapValues(\.receipt) { effects.append(saveEffect(row)) }
        rows[session] = row
        return effects
    }

    private static func acknowledge(panes: Set<UInt32>, row: inout SessionRow) -> [Effect] {
        var effects: [Effect] = []
        for id in panes.sorted() where row.panes.contains(id) {
            guard var pane = row.paneStatuses[id], pane.current, pane.status == .done || pane.status == .error,
                  pane.seq > 0, pane.acknowledged != pane.identity else { continue }
            let identity = pane.identity
            effects.append(.ack(StatusAcknowledgement(
                host: row.host, instance: row.instance, pane: id, record: identity)))
            pane.receipt.unread.remove(identity)
            pane.receipt.seen.insert(identity)
            pane.acknowledged = identity
            row.paneStatuses[id] = pane
        }
        return effects
    }

    private static func updateUnread(_ row: inout SessionRow) {
        row.unread = row.paneStatuses.values.reduce(0) { $0 + $1.receipt.unread.count }
    }

    private static func saveEffect(_ row: SessionRow) -> Effect {
        .saveStatus(SessionRef(host: row.host, session: row.id), instance: row.instance,
                    receipts: row.paneStatuses.mapValues(\.receipt))
    }

    /// A reconnect can expose an older pending record or no record at all. Cached reports
    /// remain display hints until replaced, but cannot authorize acknowledgements.
    static func awaitReports(in rows: inout [Int: SessionRow]) {
        for id in rows.keys {
            guard var row = rows[id] else { continue }
            row.paneStatuses = row.paneStatuses.mapValues { pane in
                var pane = pane
                pane.current = false
                pane.newestSeq = 0
                pane.acknowledged = nil
                return pane
            }
            rows[id] = row
        }
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
    /// `receipts` reads event identities for this host instance on the first list;
    /// `gitRoot` finds a new session's project root (nil: group by the cwd's last component).
    static func merge(
        _ listed: [Listed], into previous: [Int: SessionRow], host: String, instance: String, now: Date,
        receipts: (SessionRef) -> [UInt32: PaneStatus.Receipt], gitRoot: (String) -> String?
    ) -> [Int: SessionRow] {
        var updated: [Int: SessionRow] = [:]
        for item in listed {
            guard let first = item.panes.first else { continue }
            let candidate = previous[item.id]
            let old = candidate?.host == host && candidate?.instance == instance ? candidate : nil
            let saved = receipts(SessionRef(host: host, session: item.id))
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
                unread: 0,
                lastSeen: old?.lastSeen ?? now,
                sizeSource: old?.sizeSource ?? "",
                clients: old?.clients ?? 1,
                host: host,
                // Only a new session asks for its git root: the group is pinned after that.
                group: old.map(\.group).flatMap { $0.isEmpty ? nil : $0 }
                    ?? Grouping.pinnedGroup(existing: nil, cwd: item.cwd, gitRoot: gitRoot(item.cwd)),
                agent: old?.agent ?? .none,
                paneStatuses: old?.paneStatuses.filter { item.panes.contains($0.key) }
                    ?? saved.filter { item.panes.contains($0.key) }.mapValues {
                        PaneStatus(status: .idle, seq: 0, agent: .none, receipt: $0, current: false)
                    },
                statusPane: old?.statusPane,
                instance: instance)
            if var row = updated[item.id] {
                updateUnread(&row)
                if !row.paneStatuses.isEmpty || old?.paneStatuses.isEmpty == false { aggregate(&row) }
                updated[item.id] = row
            }
        }
        return updated
    }
}
