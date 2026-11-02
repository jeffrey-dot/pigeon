import XCTest
@testable import PigeonApp

final class SessionReducerTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_000)

    private func row(_ id: Int, panes: [UInt32], status: SidebarSession.Status = .idle) -> SessionRow {
        SessionRow(
            id: id, panes: panes, cwd: "/w", activePane: panes[0], app: "zsh", title: "session \(id)",
            status: status, seq: 0, unread: 0, lastSeen: now, group: "w")
    }

    private func report(_ pane: UInt32, _ status: SidebarSession.Status?, seq: UInt32 = 5) -> SessionReducer.Report {
        SessionReducer.Report(pane: pane, status: status, seq: seq, agent: AgentInfo(app: "claude", message: "", kind: "", since: now))
    }

    func testDoneOnScreenIsAckedNotCounted() {
        var rows = [1: row(1, panes: [10])]
        let effects = SessionReducer.apply(report(10, .done), to: &rows, host: "local", selected: 1, onScreen: true)
        XCTAssertEqual(effects, [.ack(pane: 10, seq: 5)])
        XCTAssertEqual(rows[1]?.unread, 0)
        XCTAssertEqual(rows[1]?.agent.app, "claude")
    }

    func testDoneElsewhereBecomesUnreadAndIsSaved() {
        var rows = [1: row(1, panes: [10]), 2: row(2, panes: [20])]
        let effects = SessionReducer.apply(report(20, .error), to: &rows, host: "devbox", selected: 1, onScreen: true)
        XCTAssertEqual(rows[2]?.unread, 1)
        XCTAssertEqual(effects, [.saveUnread(SessionRef(host: "devbox", session: 2), 1)])
    }

    func testABackgroundHostsSelectionIsNotLookedAt() {
        var rows = [1: row(1, panes: [10])]
        let effects = SessionReducer.apply(report(10, .done), to: &rows, host: "devbox", selected: 1, onScreen: false)
        XCTAssertEqual(rows[1]?.unread, 1)
        XCTAssertFalse(effects.contains(.ack(pane: 10, seq: 5)))
    }

    func testLiveStatesAreNeverAckedOrCounted() {
        var rows = [1: row(1, panes: [10])]
        for state in [SidebarSession.Status.working, .blocked] {
            XCTAssertEqual(SessionReducer.apply(report(10, state), to: &rows, host: "local", selected: 1, onScreen: true), [])
            XCTAssertEqual(rows[1]?.status, state)
        }
        XCTAssertEqual(rows[1]?.unread, 0)
    }

    func testUnknownPaneOrStateChangesNothingWrong() {
        var rows = [1: row(1, panes: [10], status: .working)]
        XCTAssertEqual(SessionReducer.apply(report(99, .done), to: &rows, host: "local", selected: 1, onScreen: true), [])
        _ = SessionReducer.apply(report(10, nil, seq: 9), to: &rows, host: "local", selected: 1, onScreen: true)
        XCTAssertEqual(rows[1]?.status, .working, "a state this build does not know keeps the old one")
        XCTAssertEqual(rows[1]?.seq, 9)
    }

    func testMergeKeepsWindowStateAndPinsTheGroup() {
        var old = row(1, panes: [10], status: .blocked)
        old.unread = 2
        old.group = "pigeon"
        let listed = [
            SessionReducer.Listed(id: 1, panes: [10, 11], cwd: "/elsewhere", activePane: 11, program: "vim"),
            SessionReducer.Listed(id: 2, panes: [20], cwd: "/repo/sub", activePane: 0, program: ""),
            SessionReducer.Listed(id: 3, panes: [], cwd: "/x", activePane: 0, program: ""),
        ]
        var asked: [String] = []
        let merged = SessionReducer.merge(
            listed, into: [1: old], host: "local", now: now,
            unread: { $0.session == 2 ? 4 : 0 },
            gitRoot: { asked.append($0); return "/repo" })
        XCTAssertEqual(merged[1]?.status, .blocked)
        XCTAssertEqual(merged[1]?.unread, 2)
        XCTAssertEqual(merged[1]?.group, "pigeon", "a cd never regroups")
        XCTAssertEqual(merged[1]?.app, "vim")
        XCTAssertEqual(merged[1]?.activePane, 11)
        XCTAssertEqual(merged[2]?.unread, 4, "the saved count for a session new to this run")
        XCTAssertEqual(merged[2]?.activePane, 20)
        XCTAssertEqual(merged[2]?.app, "shell")
        XCTAssertEqual(asked, ["/repo/sub"], "only a new session looks for its git root")
        XCTAssertNil(merged[3], "a session with no panes is not a row")
    }
}
