import XCTest
@testable import PigeonApp

final class SessionReducerTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_000)

    private func row(_ id: Int, panes: [UInt32], status: SidebarSession.Status = .idle) -> SessionRow {
        SessionRow(
            id: id, panes: panes, cwd: "/w", activePane: panes[0], app: "zsh", title: "session \(id)",
            status: status, seq: 0, unread: 0, lastSeen: now, group: "w", instance: "a")
    }

    private func report(_ pane: UInt32, _ status: SidebarSession.Status?, seq: UInt32 = 5) -> SessionReducer.Report {
        SessionReducer.Report(pane: pane, status: status, seq: seq, agent: AgentInfo(app: "claude", message: "", kind: "", since: now))
    }

    func testDoneOnScreenIsAckedNotCounted() {
        var rows = [1: row(1, panes: [10])]
        let effects = SessionReducer.apply(report(10, .done), to: &rows, host: "local", instance: "a", selected: 1, visiblePanes: [10, 11, 20])
        XCTAssertEqual(effects.first, .ack(ack(10)))
        XCTAssertEqual(rows[1]?.unread, 0)
        XCTAssertEqual(rows[1]?.agent.app, "claude")
    }

    func testAnIdleSiblingDoesNotHideABlockedPane() {
        var rows = [1: row(1, panes: [10, 11])]
        _ = SessionReducer.apply(report(10, .blocked), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        _ = SessionReducer.apply(report(11, .idle, seq: 0), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        XCTAssertEqual(rows[1]?.status, .blocked)
    }

    func testAVisibleSiblingAcknowledgesItsOwnPane() {
        var rows = [1: row(1, panes: [10, 11])]
        let effects = SessionReducer.apply(report(11, .done), to: &rows, host: "local", instance: "a", selected: 1, visiblePanes: [10, 11, 20])
        XCTAssertEqual(effects.first, .ack(ack(11)))
    }

    func testReplayingAFinishedReportDoesNotIncreaseUnread() {
        var rows = [1: row(1, panes: [10])]
        _ = SessionReducer.apply(report(10, .done), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        let replay = SessionReducer.apply(report(10, .done), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertEqual(replay, [])
    }

    func testReloadingSavedUnreadThenReplayingDoesNotCountTwice() throws {
        var rows = [1: row(1, panes: [10])]
        var saved = SidebarState()
        saved.connect(host: "local", instance: "a")
        let effects = SessionReducer.apply(report(10, .done), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        for case let .saveStatus(ref, instance, receipts) in effects {
            saved.save(receipts, for: ref, instance: instance)
        }
        let loaded = try JSONDecoder().decode(SidebarState.self, from: JSONEncoder().encode(saved))
        rows = SessionReducer.merge(
            [.init(id: 1, panes: [10], cwd: "/w", activePane: 10, program: "zsh")],
            into: [:], host: "local", instance: "a", now: now, receipts: { loaded.receipts(for: $0, instance: "a") }, gitRoot: { _ in nil })
        _ = SessionReducer.apply(report(10, .done), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        XCTAssertEqual(rows[1]?.unread, 1, "a restart must retain the event identity, not just its count")
    }

    func testAZoomHiddenSiblingStaysUnreadUntilItIsShown() {
        var rows = [1: row(1, panes: [10, 11])]
        let effects = SessionReducer.apply(
            report(11, .done), to: &rows, host: "local", instance: "a", selected: 1, visiblePanes: [10])
        XCTAssertEqual(effects.count, 1)
        XCTAssertEqual(SessionReducer.markSeen([10], in: 1, rows: &rows, host: "local", instance: "a"), [])
        XCTAssertEqual(rows[1]?.unread, 1)
        let shown = SessionReducer.markSeen([11], in: 1, rows: &rows, host: "local", instance: "a")
        XCTAssertEqual(shown.first, .ack(ack(11)))
        XCTAssertEqual(shown.count, 2)
        XCTAssertEqual(SessionReducer.markSeen([11], in: 1, rows: &rows, host: "local", instance: "a"), [])
    }

    func testClosingTheBlockedPaneRevealsItsWorkingSibling() {
        var rows = [1: row(1, panes: [10, 11])]
        _ = SessionReducer.apply(report(10, .blocked), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        _ = SessionReducer.apply(report(11, .working), to: &rows, host: "local", instance: "a", selected: nil, visiblePanes: [])
        rows = SessionReducer.merge(
            [.init(id: 1, panes: [11], cwd: "/w", activePane: 11, program: "zsh")],
            into: rows, host: "local", instance: "a", now: now, receipts: { _ in [:] }, gitRoot: { _ in nil })
        XCTAssertEqual(rows[1]?.status, .working)
        XCTAssertEqual(rows[1]?.statusPane, 11)
    }

    func testDoneElsewhereBecomesUnreadAndIsSaved() {
        var rows = [1: row(1, panes: [10]), 2: row(2, panes: [20])]
        rows = rows.mapValues { var row = $0; row.host = "devbox"; return row }
        let effects = SessionReducer.apply(report(20, .error), to: &rows, host: "devbox", instance: "a", selected: 1, visiblePanes: [10, 11, 20])
        XCTAssertEqual(rows[2]?.unread, 1)
        XCTAssertEqual(effects.count, 1)
    }

    func testABackgroundHostsSelectionIsNotLookedAt() {
        var rows = [1: row(1, panes: [10])]
        rows[1]?.host = "devbox"
        let effects = SessionReducer.apply(report(10, .done), to: &rows, host: "devbox", instance: "a", selected: 1, visiblePanes: [])
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertFalse(effects.contains(.ack(ack(10))))
    }

    func testLiveStatesAreNeverAckedOrCounted() {
        var rows = [1: row(1, panes: [10])]
        for state in [SidebarSession.Status.working, .blocked] {
            XCTAssertEqual(SessionReducer.apply(report(10, state), to: &rows, host: "local", instance: "a", selected: 1, visiblePanes: [10, 11, 20]), [])
            XCTAssertEqual(rows[1]?.status, state)
        }
        XCTAssertEqual(rows[1]?.unread, 0)
    }

    func testUnknownPaneOrStateChangesNothingWrong() {
        var rows = [1: row(1, panes: [10], status: .working)]
        XCTAssertEqual(SessionReducer.apply(report(99, .done), to: &rows, host: "local", instance: "a", selected: 1, visiblePanes: [10, 11, 20]), [])
        _ = SessionReducer.apply(report(10, nil, seq: 9), to: &rows, host: "local", instance: "a", selected: 1, visiblePanes: [10, 11, 20])
        XCTAssertEqual(rows[1]?.status, .working, "a state this build does not know keeps the old one")
        XCTAssertEqual(rows[1]?.seq, 0, "unknown states cannot replace a known report or its ack identity")
    }

    private func ack(_ pane: UInt32) -> StatusAcknowledgement {
        StatusAcknowledgement(host: "local", instance: "a", pane: pane,
                              record: StatusRecord(seq: 5, record: ""))
    }

    func testHostMayRevealAnOlderLiveRecordAfterClearingTheNewest() {
        var rows = [1: row(1, panes: [10])]
        _ = SessionReducer.apply(report(10, .working, seq: 4), to: &rows, host: "local", instance: "a",
                                 selected: nil, visiblePanes: [])
        _ = SessionReducer.apply(report(10, .blocked, seq: 6), to: &rows, host: "local", instance: "a",
                                 selected: nil, visiblePanes: [])
        _ = SessionReducer.apply(report(10, .working, seq: 4), to: &rows, host: "local", instance: "a",
                                 selected: nil, visiblePanes: [])
        XCTAssertEqual(rows[1]?.status, .working)
        XCTAssertEqual(rows[1]?.seq, 4)
    }

    func testMergeKeepsWindowStateAndPinsTheGroup() {
        var old = row(1, panes: [10], status: .blocked)
        old.paneStatuses[10] = PaneStatus(status: .blocked, seq: 3, agent: .none,
            receipt: .init(unread: [StatusRecord(seq: 1, record: ""), StatusRecord(seq: 2, record: "")]))
        old.group = "pigeon"
        let listed = [
            SessionReducer.Listed(id: 1, panes: [10, 11], cwd: "/elsewhere", activePane: 11, program: "vim"),
            SessionReducer.Listed(id: 2, panes: [20], cwd: "/repo/sub", activePane: 0, program: ""),
            SessionReducer.Listed(id: 3, panes: [], cwd: "/x", activePane: 0, program: ""),
        ]
        var asked: [String] = []
        let merged = SessionReducer.merge(
            listed, into: [1: old], host: "local", instance: "a", now: now,
            receipts: { $0.session == 2 ? [20: .init(unread: [StatusRecord(seq: 4, record: "task")])] : [:] },
            gitRoot: { asked.append($0); return "/repo" })
        XCTAssertEqual(merged[1]?.status, .blocked)
        XCTAssertEqual(merged[1]?.unread, 2)
        XCTAssertEqual(merged[1]?.group, "pigeon", "a cd never regroups")
        XCTAssertEqual(merged[1]?.app, "vim")
        XCTAssertEqual(merged[1]?.activePane, 11)
        XCTAssertEqual(merged[2]?.unread, 1, "the saved identities for a session new to this run")
        XCTAssertEqual(merged[2]?.activePane, 20)
        XCTAssertEqual(merged[2]?.app, "shell")
        XCTAssertEqual(asked, ["/repo/sub"], "only a new session looks for its git root")
        XCTAssertNil(merged[3], "a session with no panes is not a row")
    }
}
