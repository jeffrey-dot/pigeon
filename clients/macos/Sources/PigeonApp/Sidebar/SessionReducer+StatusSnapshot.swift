import Foundation

extension SessionReducer {
    /// Missing identities are host deletions, never evidence that this client saw them.
    static func apply(
        _ snapshot: StatusSnapshot, to rows: inout [Int: SessionRow], host: String, instance: String
    ) -> [Effect] {
        guard let id = rows.first(where: { $0.value.panes.contains(snapshot.pane) })?.key,
              var row = rows[id], row.host == host, row.instance == instance else { return [] }
        var pane = row.paneStatuses[snapshot.pane] ?? PaneStatus(status: .idle, seq: 0, agent: .none)
        if let revision = pane.revision, snapshot.revision <= revision { return [] }
        let before = pane.receipt
        pane.records = snapshot.records
        pane.revision = snapshot.revision
        pane.current = true
        pane.receipt.unread = Set(snapshot.records.filter {
            $0.finished && !pane.receipt.seen.contains($0.identity)
        }.map(\.identity))
        var effects: [Effect] = snapshot.records.filter {
            $0.finished && pane.receipt.seen.contains($0.identity)
        }.map {
            .ack(StatusAcknowledgement(host: host, instance: instance, pane: snapshot.pane, record: $0.identity))
        }
        project(&pane)
        row.paneStatuses[snapshot.pane] = pane
        aggregate(&row)
        updateUnread(&row)
        if before != pane.receipt { effects.append(saveEffect(row)) }
        rows[id] = row
        return effects
    }

    /// All pending records can be inspected independently, even if the row has one badge.
    static func pendingRecords(in row: SessionRow) -> [PendingStatus] {
        row.panes.flatMap { id -> [PendingStatus] in
            guard let pane = row.paneStatuses[id], pane.current else { return [] }
            let records = pane.records ?? (pane.seq > 0 ? [StatusSnapshot.Record(
                identity: pane.identity, status: pane.status, agent: pane.agent)] : [])
            return records.filter { !$0.finished || !pane.receipt.seen.contains($0.identity) }.map {
                PendingStatus(target: .init(host: row.host, instance: row.instance, pane: id, record: $0.identity),
                              status: $0.status, agent: $0.agent)
            }
        }.sorted { left, right in
            if left.status != right.status { return left.status < right.status }
            if left.target.pane != right.target.pane { return left.target.pane < right.target.pane }
            return left.target.record.record < right.target.record.record
        }
    }

    /// The caller supplies exactly the record displayed in the inspection panel and the
    /// same current-screen/foreground visibility evidence used by the legacy path.
    static func markSeen(
        _ target: StatusAcknowledgement, in session: Int, rows: inout [Int: SessionRow],
        visiblePanes: Set<UInt32>
    ) -> [Effect] {
        guard var row = rows[session], row.host == target.host, row.instance == target.instance,
              row.panes.contains(target.pane), visiblePanes.contains(target.pane),
              var pane = row.paneStatuses[target.pane], pane.current else { return [] }
        if pane.records == nil {
            guard pane.identity == target.record else { return [] }
            return markSeen([target.pane], in: session, rows: &rows, host: target.host, instance: target.instance)
        }
        guard let record = pane.records?.first(where: { $0.identity == target.record }),
              record.finished, !pane.receipt.seen.contains(target.record) else { return [] }
        pane.receipt.unread.remove(target.record)
        pane.receipt.seen.insert(target.record)
        project(&pane)
        row.paneStatuses[target.pane] = pane
        aggregate(&row)
        updateUnread(&row)
        rows[session] = row
        return [.ack(target), saveEffect(row)]
    }

    private static func project(_ pane: inout PaneStatus) {
        let winner = (pane.records ?? []).filter {
            !$0.finished || !pane.receipt.seen.contains($0.identity)
        }.min { left, right in
            if left.status != right.status { return left.status < right.status }
            return left.identity.record < right.identity.record
        }
        pane.status = winner?.status ?? .idle
        pane.seq = winner?.identity.seq ?? 0
        pane.record = winner?.identity.record ?? ""
        pane.agent = winner?.agent ?? .none
        pane.acknowledged = nil
    }
}
