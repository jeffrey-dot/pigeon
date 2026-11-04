import Foundation
import PigeonCore

extension StatusSnapshot {
    init?(message: OpaquePointer, now: Date = Date()) {
        guard let handle = pg_msg_status_snapshot(message) else { return nil }
        defer { pg_status_snapshot_free(handle) }
        guard let snapshot = Self.copy(handle: handle, now: now) else { return nil }
        self = snapshot
    }

    /// Strings borrow the handle, so every field is copied before the caller frees it.
    static func copy(handle: OpaquePointer, now: Date = Date()) -> StatusSnapshot? {
        var info = PgStatusSnapshotInfo()
        info.size = UInt32(MemoryLayout<PgStatusSnapshotInfo>.size)
        guard pg_status_snapshot_info(handle, &info) else { return nil }
        var records: [Record] = []
        for index in 0..<info.count {
            var raw = PgStatusSnapshotRecord()
            raw.size = UInt32(MemoryLayout<PgStatusSnapshotRecord>.size)
            guard pg_status_snapshot_at(handle, index, &raw) else { return nil }
            let status: SidebarSession.Status
            switch raw.state {
            case PG_STATE_IDLE: status = .idle
            case PG_STATE_WORKING: status = .working
            case PG_STATE_BLOCKED: status = .blocked
            case PG_STATE_DONE: status = .done
            case PG_STATE_ERROR: status = .error
            default: return nil
            }
            let kind: String
            switch raw.kind {
            case PG_KIND_NONE: kind = ""
            case PG_KIND_PERMISSION: kind = "permission"
            case PG_KIND_QUESTION: kind = "question"
            case PG_KIND_AUTH: kind = "auth"
            default: return nil
            }
            records.append(Record(
                identity: .init(seq: raw.seq, record: Wire.string(raw.id)), status: status,
                agent: AgentInfo(app: Wire.string(raw.app), message: Wire.string(raw.msg),
                                 kind: kind, ageMs: raw.age_ms, now: now)))
        }
        return StatusSnapshot(pane: info.pane_id, revision: info.revision, records: records)
    }
}
