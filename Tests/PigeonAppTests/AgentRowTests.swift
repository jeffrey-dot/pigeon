import XCTest
@testable import PigeonApp

/// What a session row says about its agent: the second line, the name, and the header badge.
final class AgentRowTests: XCTestCase {
    private func session(
        _ status: SidebarSession.Status,
        agent: AgentInfo = .none,
        title: String = "zsh",
        app: String = "zsh",
        reported: Bool = true
    ) -> SidebarSession {
        SidebarSession(
            id: 1, title: title, group: "pigeon", status: status, app: app,
            reported: reported, agent: agent)
    }

    func testABlockedAgentSaysWhatItIsWaitingFor() {
        let asking = AgentInfo(app: "claude", message: "Run cargo test?", kind: "permission")
        XCTAssertEqual(SidebarModel.detailLine(for: session(.blocked, agent: asking)), "Run cargo test?")
        let silent = AgentInfo(app: "claude", kind: "permission")
        XCTAssertEqual(SidebarModel.detailLine(for: session(.blocked, agent: silent)), "Waiting · permission")
        XCTAssertEqual(SidebarModel.detailLine(for: session(.blocked)), "Waiting for you")
    }

    func testOnlyLiveStatesGetASecondLine() {
        let busy = AgentInfo(app: "codex", message: "Refactoring the renderer")
        XCTAssertEqual(SidebarModel.detailLine(for: session(.working, agent: busy)), "Refactoring the renderer")
        XCTAssertNil(SidebarModel.detailLine(for: session(.working)), "no task text, one line")
        XCTAssertNil(SidebarModel.detailLine(for: session(.done, agent: busy)))
        XCTAssertNil(SidebarModel.detailLine(for: session(.idle, agent: busy)))
    }

    func testTheRowIsNamedByTheUserThenTheAgentThenTheProgram() {
        let agent = AgentInfo(app: "claude")
        XCTAssertEqual(SidebarModel.rowTitle(for: session(.working, agent: agent)), "claude")
        XCTAssertEqual(SidebarModel.rowTitle(for: session(.working)), "zsh")
        XCTAssertEqual(
            SidebarModel.rowTitle(for: session(.working, agent: agent, title: "api server")), "api server")
    }

    func testTheHeaderBadgeCarriesStateAgeAndReason() {
        let now = Date(timeIntervalSince1970: 10_000)
        let asking = AgentInfo(
            app: "claude", message: "Run cargo test?", kind: "permission",
            since: now.addingTimeInterval(-120))
        var blocked = session(.blocked, agent: asking)
        blocked.lastSeen = asking.since
        XCTAssertEqual(SidebarModel.headerBadge(for: blocked, now: now), "Needs you · 2m · Run cargo test?")
        XCTAssertEqual(SidebarModel.headerBadge(for: session(.working, reported: false), now: now), "")
        XCTAssertEqual(SidebarModel.headerBadge(for: session(.idle), now: now), "")
    }

    func testTheHostReportsAnAgeSoClocksNeedNotAgree() {
        let now = Date(timeIntervalSince1970: 1_000)
        let info = AgentInfo(app: "pi", message: "", kind: "", ageMs: 90_000, now: now)
        XCTAssertEqual(info.since, now.addingTimeInterval(-90))
        let unknown = AgentInfo(app: "pi", message: "", kind: "", ageMs: nil, now: now)
        XCTAssertEqual(unknown.since, now, "an older host: arrival time stands in")
    }
}
