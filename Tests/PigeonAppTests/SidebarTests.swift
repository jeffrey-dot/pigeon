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
