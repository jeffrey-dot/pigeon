import XCTest
@testable import PigeonApp

final class GroupingTests: XCTestCase {
    func testGitRootWinsOverTheCwd() {
        XCTAssertEqual(
            Grouping.groupName(forCwd: "/Users/me/pigeon/core", gitRoot: "/Users/me/pigeon"),
            "pigeon"
        )
    }

    func testFallsBackToTheLastCwdComponent() {
        XCTAssertEqual(Grouping.groupName(forCwd: "/Users/me/work/api"), "api")
        XCTAssertEqual(Grouping.groupName(forCwd: "/Users/me/work/api/"), "api")
        XCTAssertEqual(Grouping.groupName(forCwd: "/"), "/")
    }
}

final class SidebarModelTests: XCTestCase {
    private func session(
        _ id: Int,
        _ status: SidebarSession.Status,
        group: String = "pigeon",
        title: String? = nil,
        unread: Int = 0
    ) -> SidebarSession {
        SidebarSession(
            id: id,
            title: title ?? "session \(id)",
            group: group,
            status: status,
            unread: unread
        )
    }

    func testRowsAreOrderedByUrgencyThenTitle() {
        let sessions = [
            session(1, .idle, title: "c"),
            session(2, .blocked, title: "b"),
            session(3, .working, title: "a"),
            session(4, .error, title: "d"),
        ]
        let sections = SidebarModel.sections(sessions)
        XCTAssertEqual(sections.count, 1)
        XCTAssertEqual(sections[0].sessions.map(\.id), [2, 4, 3, 1])
    }

    func testGroupsKeepTheirFirstSeenOrder() {
        let sessions = [
            session(1, .idle, group: "beta"),
            session(2, .idle, group: "alpha"),
            session(3, .blocked, group: "beta"),
        ]
        let sections = SidebarModel.sections(sessions)
        XCTAssertEqual(sections.map(\.group), ["beta", "alpha"])
        XCTAssertEqual(sections[0].sessions.map(\.id), [3, 1])
    }

    func testNeedsYouCountsBlockedErrorsAndUnreadDone() {
        let sessions = [
            session(1, .blocked),
            session(2, .error),
            session(3, .done, unread: 2),
            session(4, .done, unread: 0),
            session(5, .working),
            session(6, .idle),
        ]
        XCTAssertEqual(SidebarModel.needsYou(sessions), 3)
    }

    func testSubtitleMatchesTheReferenceCopy() {
        let now = Date(timeIntervalSince1970: 1_000)
        let row = session(1, .working)
        XCTAssertEqual(
            SidebarModel.subtitle(for: row, now: now, since: now.addingTimeInterval(-90)),
            "Working · 1m"
        )
    }
}

final class SidebarOrderingTests: XCTestCase {
    private func row(_ id: Int, _ status: SidebarSession.Status, group: String = "pigeon", app: String = "zsh") -> SidebarSession {
        SidebarSession(id: id, title: "session \(id)", group: group, status: status, unread: 0, app: app, cwd: "/tmp/pigeon")
    }

    func testBlockedComesFirstThenPersistedOrder() {
        let sessions = [row(1, .idle), row(2, .blocked), row(3, .working), row(4, .blocked)]
        let sections = SidebarModel.sections(sessions, order: [4, 1, 3, 2])
        // blocked first (4 before 2 by the persisted order), then working, then idle.
        XCTAssertEqual(sections[0].sessions.map(\.id), [4, 2, 3, 1])
    }

    func testNextNeedsYouCyclesThroughBlockedAndUnreadDone() {
        var done = row(5, .done)
        done.unread = 2
        let sessions = [row(1, .idle), row(2, .blocked), row(3, .working), done]
        XCTAssertEqual(SidebarModel.nextNeedsYou(sessions, after: nil)?.id, 2)
        XCTAssertEqual(SidebarModel.nextNeedsYou(sessions, after: 2)?.id, 5)
        XCTAssertEqual(SidebarModel.nextNeedsYou(sessions, after: 5)?.id, 2, "wraps")
        XCTAssertNil(SidebarModel.nextNeedsYou([row(7, .idle)], after: nil))
    }

    func testRowSubtitleShowsTheActiveApplication() {
        var session = row(1, .working, app: "vim")
        session.lastSeen = Date(timeIntervalSince1970: 1_000)
        let now = Date(timeIntervalSince1970: 1_090)
        XCTAssertEqual(SidebarModel.rowSubtitle(for: session, now: now), "vim · Working · 1m")
    }

    func testStatusParsingCoversTheProtocolStrings() {
        XCTAssertEqual(SidebarModel.status(from: "blocked"), .blocked)
        XCTAssertEqual(SidebarModel.status(from: "error"), .error)
        XCTAssertNil(SidebarModel.status(from: "spinning"))
    }
}

final class SidebarStoreTests: XCTestCase {
    private var url: URL!

    override func setUp() {
        super.setUp()
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-sidebar-\(UUID().uuidString).json")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: url)
        super.tearDown()
    }

    func testRoundTripAndAtomicFile() {
        let store = SidebarStore(url: url)
        var state = SidebarState()
        state.collapsed = ["pigeon"]
        state.order = [3, 1, 2]
        state.renamed["3"] = "deploy"
        store.save(state)
        XCTAssertEqual(store.load(), state)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
        // The state file lives under ~/.config/pigeon, never ~/Library.
        XCTAssertTrue(SidebarStore.defaultURL.path.hasPrefix(
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".config/pigeon").path
        ))
    }

    func testCorruptFileFallsBackToDefaults() {
        try? Data("{ not json".utf8).write(to: url)
        XCTAssertEqual(SidebarStore(url: url).load(), SidebarState())
    }
}
