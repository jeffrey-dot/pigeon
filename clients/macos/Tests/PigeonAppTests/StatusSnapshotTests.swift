import XCTest
@testable import PigeonApp

final class StatusSnapshotTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_000)

    private func rows(instance: String = "a") -> [Int: SessionRow] {
        SessionReducer.merge(
            [.init(id: 1, panes: [10, 11], cwd: "/project", activePane: 10, program: "zsh")],
            into: [:], host: "devbox", instance: instance, now: now,
            receipts: { _ in [:] }, gitRoot: { _ in nil })
    }

    private func record(_ id: String, _ seq: UInt32, _ status: SidebarSession.Status) -> StatusSnapshot.Record {
        .init(identity: .init(seq: seq, record: id), status: status,
              agent: AgentInfo(app: "agent", message: id, kind: "", since: now))
    }

    @discardableResult
    private func apply(_ records: [StatusSnapshot.Record], revision: UInt64 = 1,
                       to rows: inout [Int: SessionRow]) -> [SessionReducer.Effect] {
        SessionReducer.apply(.init(pane: 10, revision: revision, records: records),
                             to: &rows, host: "devbox", instance: "a")
    }

    func testTwoBlockedAndDoneRemainIndividuallySelectableInOnePane() throws {
        var rows = rows()
        let effects = apply([record("question-b", 3, .blocked), record("finished", 9, .done),
                             record("question-a", 2, .blocked)], to: &rows)
        let row = try XCTUnwrap(rows[1])
        XCTAssertEqual(row.status, .blocked)
        XCTAssertEqual(row.unread, 1)
        XCTAssertEqual(row.statusPane, 10)
        XCTAssertEqual(row.statusRecord, .init(seq: 2, record: "question-a"))
        let pending = SessionReducer.pendingRecords(in: row)
        XCTAssertEqual(pending.map(\.target.record.record), ["question-a", "question-b", "finished"])
        XCTAssertEqual(pending.map(\.agent.message), ["question-a", "question-b", "finished"])
        XCTAssertFalse(effects.contains { if case .ack = $0 { return true }; return false })
        XCTAssertTrue(SessionReducer.markSeen([10], in: 1, rows: &rows, host: "devbox", instance: "a").isEmpty,
                      "a visible pane does not present every record inside it")
        XCTAssertEqual(rows[1]?.unread, 1)
    }
    private func acks(_ effects: [SessionReducer.Effect]) -> [StatusAcknowledgement] {
        effects.compactMap { if case let .ack(target) = $0 { return target }; return nil }
    }

    func testConfirmingOneDisplayedTerminalLeavesEveryOtherRecordPending() throws {
        var rows = rows()
        apply([record("a", 1, .error), record("b", 2, .done), record("live", 3, .blocked)], to: &rows)
        let targets = SessionReducer.pendingRecords(in: try XCTUnwrap(rows[1])).map(\.target)
        let done = try XCTUnwrap(targets.first { $0.record.record == "b" })
        XCTAssertTrue(SessionReducer.markSeen(done, in: 1, rows: &rows, visiblePanes: []).isEmpty)
        let effects = SessionReducer.markSeen(done, in: 1, rows: &rows, visiblePanes: [10])
        XCTAssertEqual(acks(effects), [done])
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertEqual(SessionReducer.pendingRecords(in: try XCTUnwrap(rows[1])).map(\.target.record.record),
                       ["live", "a"])
        XCTAssertEqual(rows[1]?.paneStatuses[10]?.receipt.seen, [done.record])
        XCTAssertTrue(SessionReducer.markSeen(done, in: 1, rows: &rows, visiblePanes: [10]).isEmpty)
        XCTAssertTrue(SessionReducer.markSeen(targets[0], in: 1, rows: &rows, visiblePanes: [10]).isEmpty)
        var wrong = done
        wrong.host = "local"
        XCTAssertTrue(SessionReducer.markSeen(wrong, in: 1, rows: &rows, visiblePanes: [10]).isEmpty)
    }

    func testClearRemovesPendingWithoutInventingSeenAndEmptyResetsProjection() throws {
        var rows = rows()
        apply([record("keep", 1, .done), record("remove", 2, .error)], to: &rows)
        let removed = try XCTUnwrap(SessionReducer.pendingRecords(in: rows[1]!).first?.target)
        let effects = apply([record("keep", 1, .done)], revision: 2, to: &rows)
        XCTAssertTrue(acks(effects).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertEqual(rows[1]?.paneStatuses[10]?.receipt.seen, [])
        XCTAssertTrue(SessionReducer.markSeen(removed, in: 1, rows: &rows, visiblePanes: [10]).isEmpty)
        XCTAssertTrue(acks(apply([], revision: 3, to: &rows)).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 0)
        XCTAssertEqual(rows[1]?.status, .idle)
        XCTAssertNil(rows[1]?.statusRecord)
        XCTAssertNil(rows[1]?.statusPane)
        XCTAssertEqual(rows[1]?.seq, 0)
        XCTAssertEqual(rows[1]?.paneStatuses[10]?.receipt.seen, [])
        XCTAssertTrue(SessionReducer.pendingRecords(in: rows[1]!).isEmpty)
        apply([record("remove", 2, .error)], revision: 4, to: &rows)
        XCTAssertEqual(rows[1]?.unread, 1, "host deletion never supplies local deduplication evidence")
    }

    func testRevisionIsIndependentOfRecordSeqAndRejectsOlderOrDuplicateSets() {
        var rows = rows()
        apply([record("newer-record", 90, .blocked)], revision: 5, to: &rows)
        apply([record("older-record", 2, .done)], revision: 6, to: &rows)
        XCTAssertEqual(rows[1]?.status, .done)
        XCTAssertEqual(rows[1]?.seq, 2)
        let before = rows
        XCTAssertTrue(apply([], revision: 5, to: &rows).isEmpty)
        XCTAssertTrue(apply([], revision: 6, to: &rows).isEmpty)
        XCTAssertEqual(rows, before)
    }

    func testReconnectAcceptsSameRevisionFirstSetButDoesNotTrustCachedReports() throws {
        var rows = rows()
        apply([record("done", 2, .done)], revision: 8, to: &rows)
        let target = try XCTUnwrap(SessionReducer.pendingRecords(in: rows[1]!).first?.target)
        SessionReducer.awaitReports(in: &rows)
        XCTAssertTrue(SessionReducer.pendingRecords(in: rows[1]!).isEmpty)
        XCTAssertTrue(SessionReducer.markSeen(target, in: 1, rows: &rows, visiblePanes: [10]).isEmpty)
        apply([record("done", 2, .done)], revision: 8, to: &rows)
        XCTAssertEqual(SessionReducer.pendingRecords(in: rows[1]!).count, 1)
        XCTAssertEqual(acks(SessionReducer.markSeen(target, in: 1, rows: &rows, visiblePanes: [10])), [target])
    }

    func testNewInstanceAndOtherHostsCannotUseOldRecords() throws {
        var rows = rows()
        apply([record("done", 2, .done)], to: &rows)
        let target = try XCTUnwrap(SessionReducer.pendingRecords(in: rows[1]!).first?.target)
        rows = self.rows(instance: "b")
        XCTAssertTrue(apply([record("done", 2, .done)], to: &rows).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 0)
        _ = SessionReducer.apply(StatusSnapshot(pane: 10, revision: 1, records: [record("done", 2, .done)]),
                                 to: &rows, host: "devbox", instance: "b")
        XCTAssertTrue(SessionReducer.markSeen(target, in: 1, rows: &rows, visiblePanes: [10]).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertTrue(SessionReducer.apply(StatusSnapshot(pane: 10, revision: 99, records: []),
                                            to: &rows, host: "local", instance: "b").isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1)
    }

    func testRestartDeduplicatesSeenButMissingRecordsNeverBecomeSeenOnDisk() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = SidebarStore(url: directory.appendingPathComponent("sidebar.json"))
        var state = SidebarState()
        state.connect(host: "devbox", instance: "a")
        var rows = rows()
        apply([record("seen", 1, .error), record("deleted", 2, .done), record("unread", 3, .done)], to: &rows)
        let target = try XCTUnwrap(SessionReducer.pendingRecords(in: rows[1]!).first?.target)
        _ = SessionReducer.markSeen(target, in: 1, rows: &rows, visiblePanes: [10])
        apply([record("seen", 1, .error), record("unread", 3, .done)], revision: 2, to: &rows)
        state.retain(rows, host: "devbox", instance: "a")
        store.save(state)
        let loaded = store.load()
        rows = SessionReducer.merge(
            [.init(id: 1, panes: [10], cwd: "/project", activePane: 10, program: "zsh")], into: [:],
            host: "devbox", instance: "a", now: now,
            receipts: { loaded.receipts(for: $0, instance: "a") }, gitRoot: { _ in nil })
        let replay = apply([record("seen", 1, .error), record("unread", 3, .done)], revision: 2, to: &rows)
        XCTAssertEqual(acks(replay), [target])
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertEqual(rows[1]?.paneStatuses[10]?.receipt.seen, [target.record])
        XCTAssertTrue(apply([], revision: 2, to: &rows).isEmpty)
    }

    func testInspectionKeepsExactSelectionAndDoesNotSubstituteDeletedOrUpdatedRecords() throws {
        var rows = rows()
        apply([record("a", 1, .blocked), record("b", 2, .done)], to: &rows)
        let generation = UUID()
        var inspection = StatusInspection(session: .init(host: "devbox", session: 1),
                                          instance: "a", generation: generation)
        let items = SessionReducer.pendingRecords(in: rows[1]!)
        inspection.update(items)
        inspection.select(items[1].target)
        XCTAssertEqual(inspection.selected?.target.record.record, "b")
        apply([record("a", 1, .blocked), record("b", 3, .done)], revision: 2, to: &rows)
        inspection.update(SessionReducer.pendingRecords(in: rows[1]!))
        XCTAssertNil(inspection.selected, "a reused id with another seq has not been displayed")
        inspection.select(items[1].target)
        XCTAssertNil(inspection.selected, "a stale menu item cannot select a removed identity")
        XCTAssertTrue(inspection.matches(host: "devbox", session: 1, instance: "a", generation: generation))
        XCTAssertFalse(inspection.matches(host: "devbox", session: 1, instance: "a", generation: UUID()))
        XCTAssertFalse(inspection.matches(host: "devbox", session: 1, instance: "b", generation: generation))
        XCTAssertFalse(inspection.matches(host: "local", session: 1, instance: "a", generation: generation))
    }

    func testEmptyPaneDoesNotHideAnotherPanesExplicitIdleRecord() {
        var rows = rows()
        apply([], to: &rows)
        _ = SessionReducer.apply(StatusSnapshot(pane: 11, revision: 1, records: [record("idle", 1, .idle)]),
                                 to: &rows, host: "devbox", instance: "a")
        XCTAssertEqual(rows[1]?.status, .idle)
        XCTAssertEqual(rows[1]?.statusPane, 11)
        XCTAssertEqual(rows[1]?.statusRecord, .init(seq: 1, record: "idle"))
    }

    func testLegacyFallbackReplacesCachedCollectionAndOnlyConfirmsItsCurrentReport() throws {
        var rows = rows()
        apply([record("previous", 1, .done)], to: &rows)
        SessionReducer.awaitReports(in: &rows)
        _ = SessionReducer.apply(SessionReducer.Report(pane: 10, status: .done, seq: 2,
                                                       agent: .none, record: "current"),
                                 to: &rows, host: "devbox", instance: "a", selected: nil, visiblePanes: [])
        XCTAssertEqual(rows[1]?.unread, 2)
        let current = try XCTUnwrap(SessionReducer.pendingRecords(in: rows[1]!).first?.target)
        XCTAssertEqual(current.record.record, "current")
        XCTAssertEqual(acks(SessionReducer.markSeen(current, in: 1, rows: &rows, visiblePanes: [10])), [current])
        XCTAssertEqual(rows[1]?.unread, 1, "legacy cannot infer deletion from another record")
    }

}
