import XCTest
@testable import PigeonApp

final class StatusPersistenceTests: XCTestCase {
    private let ref = SessionRef(host: "devbox", session: 1)
    private let now = Date(timeIntervalSince1970: 1_000)

    private func listed(_ panes: [UInt32] = [10, 11]) -> [SessionReducer.Listed] {
        [.init(id: 1, panes: panes, cwd: "/project", activePane: 10, program: "zsh")]
    }

    private func restored(_ state: SidebarState, instance: String = "a") -> [Int: SessionRow] {
        SessionReducer.merge(listed(), into: [:], host: ref.host, instance: instance, now: now,
                             receipts: { state.receipts(for: $0, instance: instance) }, gitRoot: { _ in nil })
    }

    @discardableResult
    private func apply(
        _ status: SidebarSession.Status, seq: UInt32, record: String = "task", pane: UInt32 = 10,
        to rows: inout [Int: SessionRow], instance: String = "a", visible: Set<UInt32> = []
    ) -> [SessionReducer.Effect] {
        SessionReducer.apply(.init(pane: pane, status: status, seq: seq, agent: .none, record: record),
                             to: &rows, host: ref.host, instance: instance, selected: 1, visiblePanes: visible)
    }

    private func save(_ effects: [SessionReducer.Effect], in state: inout SidebarState) {
        for case let .saveStatus(ref, instance, receipts) in effects {
            state.save(receipts, for: ref, instance: instance)
        }
    }

    private func acks(_ effects: [SessionReducer.Effect]) -> [StatusAcknowledgement] {
        effects.compactMap { if case let .ack(target) = $0 { return target }; return nil }
    }

    func testFirstSaveAndOverwriteSurviveARealStoreReload() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("sidebar.json")
        var state = SidebarState()
        state.connect(host: ref.host, instance: "a")
        var rows = restored(state)
        save(apply(.done, seq: 5, to: &rows), in: &state)
        SidebarStore(url: url).save(state)
        XCTAssertEqual(SidebarStore(url: url).load(), state)
        var loaded = SidebarStore(url: url).load()
        rows = restored(loaded)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertTrue(acks(SessionReducer.markSeen([10], in: 1, rows: &rows,
                                                  host: ref.host, instance: "a")).isEmpty,
                      "loading identities alone is not a displayed report")
        save(apply(.done, seq: 5, to: &rows), in: &loaded)
        XCTAssertEqual(rows[1]?.unread, 1)
        save(SessionReducer.markSeen([10], in: 1, rows: &rows, host: ref.host, instance: "a"), in: &loaded)
        SidebarStore(url: url).save(loaded)
        XCTAssertEqual(SidebarStore(url: url).load(), loaded)
        rows = restored(SidebarStore(url: url).load())
        let replay = apply(.done, seq: 5, to: &rows)
        XCTAssertEqual(rows[1]?.unread, 0)
        XCTAssertEqual(acks(replay), [.init(host: ref.host, instance: "a", pane: 10,
                                           record: .init(seq: 5, record: "task"))],
                       "an exact already-viewed identity can retry a lost ack after restart")
    }

    func testSeeingTheLatestRecordNeverAcknowledgesEarlierUnreadRecords() {
        var rows = restored(SidebarState())
        apply(.error, seq: 3, record: "first", to: &rows)
        apply(.done, seq: 8, record: "second", to: &rows)
        XCTAssertEqual(rows[1]?.unread, 2)
        let seen = SessionReducer.markSeen([10], in: 1, rows: &rows, host: ref.host, instance: "a")
        XCTAssertEqual(acks(seen).map(\.record.seq), [8])
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertTrue(SessionReducer.markSeen([10], in: 1, rows: &rows,
                                              host: ref.host, instance: "a").isEmpty)
        let earlier = apply(.error, seq: 3, record: "first", to: &rows, visible: [10])
        XCTAssertEqual(acks(earlier).map(\.record.seq), [3], "the host can reveal an older pending record")
        XCTAssertEqual(rows[1]?.unread, 0)
    }

    func testAnAlreadySeenReportRetriesAckWithoutCreatingUnread() {
        var rows = restored(SidebarState())
        apply(.done, seq: 8, record: "finished", to: &rows, visible: [10])
        apply(.blocked, seq: 3, record: "pending", to: &rows)
        XCTAssertEqual(rows[1]?.status, .blocked)
        let replay = apply(.done, seq: 8, record: "finished", to: &rows)
        XCTAssertEqual(acks(replay).map(\.record.seq), [8])
        XCTAssertEqual(rows[1]?.status, .done, "current state follows the host's ordered report")
        XCTAssertEqual(rows[1]?.seq, 8)
        XCTAssertEqual(rows[1]?.unread, 0)
    }

    func testLowerSequenceTerminalIsRetainedAndBecomesCurrentWithoutImplicitAck() {
        var rows = restored(SidebarState())
        apply(.blocked, seq: 9, to: &rows)
        let older = apply(.error, seq: 2, record: "older", to: &rows)
        XCTAssertTrue(acks(older).isEmpty)
        XCTAssertEqual(rows[1]?.status, .error)
        XCTAssertEqual(rows[1]?.unread, 1)
        apply(.error, seq: 2, record: "older", to: &rows)
        XCTAssertEqual(rows[1]?.unread, 1)
        apply(.idle, seq: 0, record: "", to: &rows, visible: [10])
        XCTAssertEqual(rows[1]?.status, .idle)
        XCTAssertEqual(rows[1]?.unread, 1, "a clear is not evidence the user saw a terminal record")
        let revealed = apply(.error, seq: 2, record: "older", to: &rows, visible: [10])
        XCTAssertEqual(acks(revealed).map(\.record.seq), [2])
    }

    func testLiveUpdatesDoNotAcknowledgeEarlierFinishedRecords() {
        var rows = restored(SidebarState())
        apply(.done, seq: 1, to: &rows)
        apply(.working, seq: 2, to: &rows)
        XCTAssertTrue(acks(SessionReducer.markSeen([10], in: 1, rows: &rows,
                                                  host: ref.host, instance: "a")).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertEqual(rows[1]?.status, .working)
    }

    func testPromptClearingBlockedRevealsTheHostsOlderUnreadDone() {
        var rows = restored(SidebarState())
        apply(.done, seq: 1, record: "finished", to: &rows)
        apply(.blocked, seq: 2, record: "running", to: &rows)
        // OSC 133A removes the transient record; the host's current report becomes done/1.
        let revealed = apply(.done, seq: 1, record: "finished", to: &rows)
        XCTAssertEqual(rows[1]?.status, .done)
        XCTAssertEqual(rows[1]?.seq, 1)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertTrue(acks(revealed).isEmpty, "receiving the report does not mean it was viewed")
    }

    func testReconnectKeepsUnreadButRequiresAFreshReportBeforeAcknowledgement() {
        var rows = restored(SidebarState())
        apply(.done, seq: 9, to: &rows)
        SessionReducer.awaitReports(in: &rows)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertTrue(acks(SessionReducer.markSeen([10], in: 1, rows: &rows,
                                                  host: ref.host, instance: "a")).isEmpty)
        let older = apply(.error, seq: 2, record: "older", to: &rows, visible: [10])
        XCTAssertEqual(acks(older).map(\.record.seq), [2])
        XCTAssertEqual(rows[1]?.unread, 1, "a missing replay cannot silently acknowledge the old record")
    }

    func testAHostRestartDoesNotReuseReadOrUnreadIdentities() {
        var state = SidebarState()
        state.connect(host: ref.host, instance: "a")
        var rows = restored(state)
        save(apply(.done, seq: 1, to: &rows, visible: [10]), in: &state)
        save(apply(.error, seq: 2, to: &rows), in: &state)
        state.connect(host: ref.host, instance: "b")
        rows = SessionReducer.merge(listed(), into: rows, host: ref.host, instance: "b", now: now,
                                    receipts: { state.receipts(for: $0, instance: "b") }, gitRoot: { _ in nil })
        XCTAssertEqual(rows[1]?.unread, 0)
        XCTAssertEqual(rows[1]?.seq, 0)
        XCTAssertTrue(apply(.error, seq: 7, to: &rows, instance: "a").isEmpty)
        let reused = apply(.done, seq: 1, to: &rows, instance: "b")
        save(reused, in: &state)
        XCTAssertTrue(acks(reused).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1)
        state.save([10: .init(seen: [.init(seq: 1, record: "task")])], for: ref, instance: "a")
        XCTAssertEqual(restored(state, instance: "b")[1]?.unread, 1, "a delayed save is instance-scoped too")
    }

    func testRecordAndPaneIdentityAreIndependentAndAckKeepsItsHost() {
        var rows = restored(SidebarState())
        apply(.done, seq: 4, record: "first", to: &rows)
        apply(.done, seq: 4, record: "second", to: &rows)
        apply(.error, seq: 4, record: "first", pane: 11, to: &rows)
        XCTAssertEqual(rows[1]?.unread, 3)
        XCTAssertEqual(rows[1]?.statusPane, 11)
        let seen = SessionReducer.markSeen([10], in: 1, rows: &rows, host: ref.host, instance: "a")
        XCTAssertEqual(acks(seen), [.init(host: ref.host, instance: "a", pane: 10,
                                        record: .init(seq: 4, record: "second"))])
        XCTAssertEqual(rows[1]?.unread, 2)
        XCTAssertFalse(acks(seen)[0].matches(host: "local", instance: "a"))
        XCTAssertFalse(acks(seen)[0].matches(host: ref.host, instance: "b"))
    }

    func testLegacyCountsKeepPreferencesButCannotBeAssignedToReusedPaneIDs() throws {
        let json = #"{"collapsed":["x"],"order":[100003,200002],"renamed":{"devbox:1":"deploy"},"unread":{"devbox:1":4},"renamedGroups":{"x":"project"}}"#
        var state = try JSONDecoder().decode(SidebarState.self, from: Data(json.utf8))
        state.connect(host: ref.host, instance: "new")
        var rows = restored(state, instance: "new")
        XCTAssertEqual(rows[1]?.unread, 0)
        XCTAssertEqual(state.unread[ref.key], 4, "unidentifiable legacy counts are retained, not treated as seen")
        XCTAssertEqual(state.order, ["3"])
        XCTAssertEqual(state.collapsed, ["x"])
        XCTAssertEqual(state.renamed[ref.key], "deploy")
        XCTAssertEqual(state.renamedGroups["x"], "project")
        XCTAssertTrue(acks(apply(.done, seq: 1, to: &rows, instance: "new")).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1, "only the replay supplies an attributable record")
    }

    func testMalformedStateKeepsOtherPreferencesAndNeverInventsSeenRecords() throws {
        let json = #"{"renamed":{"1":"keep"},"statusHosts":"invalid","unread":{"1":-4}}"#
        var state = try JSONDecoder().decode(SidebarState.self, from: Data(json.utf8))
        state.connect(host: ref.host, instance: "a")
        var rows = restored(state)
        XCTAssertEqual(state.renamed["1"], "keep")
        XCTAssertTrue(acks(apply(.done, seq: 1, to: &rows)).isEmpty)
        XCTAssertEqual(rows[1]?.unread, 1)
    }

    func testRemovingAPanePrunesOnlyItsReceiptsOnTheCorrectHost() {
        var state = SidebarState()
        state.connect(host: ref.host, instance: "a")
        state.connect(host: "ada@[::1]:2222", instance: "a")
        let other = SessionRef(host: "ada@[::1]:2222", session: 1)
        state.save([10: .init(unread: [.init(seq: 1, record: "remote")])], for: other, instance: "a")
        var rows = restored(state)
        save(apply(.done, seq: 1, to: &rows), in: &state)
        save(apply(.error, seq: 2, pane: 11, to: &rows), in: &state)
        rows = SessionReducer.merge(listed([11]), into: rows, host: ref.host, instance: "a", now: now,
                                    receipts: { state.receipts(for: $0, instance: "a") }, gitRoot: { _ in nil })
        state.retain(rows, host: ref.host, instance: "a")
        XCTAssertNil(state.receipts(for: ref, instance: "a")[10])
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertEqual(state.receipts(for: other, instance: "a")[10]?.unread.count, 1)
    }
}
