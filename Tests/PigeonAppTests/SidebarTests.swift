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

    func testClosingTheLastSessionQuitsUnlessOneIsBeingCreated() {
        XCTAssertTrue(SidebarModel.shouldQuitAfterClose(remainingSessions: 0, creatingSession: false))
        XCTAssertFalse(SidebarModel.shouldQuitAfterClose(remainingSessions: 1, creatingSession: false))
        XCTAssertFalse(
            SidebarModel.shouldQuitAfterClose(remainingSessions: 0, creatingSession: true),
            "a ⌘T answer is on its way")
    }

    func testRowSubtitleShowsTheActiveApplication() {
        var session = row(1, .working, app: "vim")
        session.lastSeen = Date(timeIntervalSince1970: 1_000)
        let now = Date(timeIntervalSince1970: 1_090)
        XCTAssertEqual(SidebarModel.rowSubtitle(for: session, now: now), "Working · 1m")
    }

    func testSubtitleWordingForTheHeaderBadge() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        XCTAssertEqual(
            SidebarModel.subtitle(status: .blocked, since: now.addingTimeInterval(-120), now: now),
            "Needs you · 2m"
        )
        XCTAssertEqual(SidebarModel.subtitle(status: .idle, since: now, now: now), "Idle · now")
    }

    func testAppKindMapsNamesToHarpLikeIcons() {
        XCTAssertEqual(SidebarModel.appKind("pi"), .agent)
        XCTAssertEqual(SidebarModel.appKind("nvim"), .editor)
        XCTAssertEqual(SidebarModel.appKind("/bin/zsh"), .shell)
        XCTAssertEqual(SidebarModel.appKind("htop"), .other)
        // The leading circle must be stable across launches for the same project.
        XCTAssertEqual(SidebarModel.tintIndex(cwd: "/Users/me/pigeon"), SidebarModel.tintIndex(cwd: "/Users/me/pigeon"))
        let tints = (0..<8).map { SidebarModel.tintIndex(cwd: "/tmp/project-\($0)") }
        XCTAssertTrue(tints.allSatisfy { (0..<6).contains($0) })
    }

    func testGroupNamesCapitalizeOnlyTheFirstCharacter() {
        XCTAssertEqual(SidebarModel.displayGroupName("atlas"), "Atlas")
        XCTAssertEqual(SidebarModel.displayGroupName("my app"), "My app")
        XCTAssertEqual(SidebarModel.displayGroupName(""), "")
    }

    func testRelativeTimeBucketsMatchTheSpec() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        func age(_ seconds: TimeInterval) -> String {
            SidebarModel.relativeTime(now.addingTimeInterval(-seconds), now: now)
        }
        XCTAssertEqual(age(0), "now")
        XCTAssertEqual(age(59), "now")
        XCTAssertEqual(age(60), "1m")
        XCTAssertEqual(age(3_599), "59m")
        XCTAssertEqual(age(3_600), "1h")
        XCTAssertEqual(age(90_000), "1d")
    }

    func testAgeIsHiddenUntilThePaneReports() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        var quiet = row(1, .idle)
        quiet.lastSeen = now.addingTimeInterval(-600)
        XCTAssertEqual(SidebarModel.ageText(for: quiet, now: now), "")
        quiet.reported = true
        XCTAssertEqual(SidebarModel.ageText(for: quiet, now: now), "10m")
    }

    func testAgeNamesTheSizeOwnerOnlyWhenMoreThanOneWindowWatches() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        var shared = row(1, .working)
        shared.reported = true
        shared.lastSeen = now.addingTimeInterval(-600)
        // One window: the age alone, exactly as before.
        XCTAssertEqual(SidebarModel.ageText(for: shared, now: now), "10m")
        shared.clients = 2
        shared.sizeSource = "latest"
        XCTAssertEqual(SidebarModel.ageText(for: shared, now: now), "10m · 2× latest")
        // Nothing reported yet: the size owner is still worth showing.
        shared.reported = false
        XCTAssertEqual(SidebarModel.ageText(for: shared, now: now), "2× latest")
    }

    func testAGroupIsPinnedWhenTheSessionAppears() {
        // The first cwd decides the group; a later `cd` must not move the row.
        XCTAssertEqual(Grouping.pinnedGroup(existing: "api", cwd: "/Users/x/dev/y"), "api")
        XCTAssertEqual(Grouping.pinnedGroup(existing: nil, cwd: "/Users/x/dev"), "dev")
        XCTAssertEqual(Grouping.pinnedGroup(existing: "", cwd: "/home/x"), "~")
        XCTAssertEqual(Grouping.pinnedGroup(existing: nil, cwd: ""), "~")
        XCTAssertEqual(
            Grouping.pinnedGroup(existing: nil, cwd: "/home/x/proj/src", gitRoot: "/home/x/proj"),
            "proj", "a git root still names the group when the session first appears")
    }

    func testHomeDirectoriesBecomeTildeOnEveryPlatform() {
        XCTAssertEqual(Grouping.groupName(forCwd: "/Users/jeffrey"), "~")
        XCTAssertEqual(Grouping.groupName(forCwd: "/home/jeffrey"), "~")
        XCTAssertEqual(Grouping.groupName(forCwd: "/home/jeffrey/"), "~")
        XCTAssertEqual(Grouping.groupName(forCwd: "~"), "~")
        XCTAssertEqual(Grouping.groupName(forCwd: "/home/jeffrey/dev/pigeon"), "pigeon")
        XCTAssertEqual(Grouping.groupName(forCwd: "/Users/jeffrey/dev/pigeon"), "pigeon")
        XCTAssertEqual(Grouping.groupName(forCwd: "/tmp"), "tmp")
        XCTAssertEqual(
            Grouping.groupName(forCwd: "/home/jeffrey/dev/pigeon", gitRoot: "/home/jeffrey/dev/pigeon/core"),
            "core", "a git root still wins over the cwd's own name")
    }

    func testGroupsSitUnderTheirMachineAndShowOnlyTheProject() {
        var remote = row(1, .idle)
        remote.group = "~"
        remote.host = "arch"
        let section = SidebarModel.sections([remote]).first
        XCTAssertEqual(section?.host, "arch")
        XCTAssertEqual(section?.group, "~", "the machine has its own heading now")
        XCTAssertEqual(section?.key, "arch / ~", "renames stay per machine")
    }

    func testRenameKeysAreProjectAloneLocallyAndHostPrefixedRemotely() {
        XCTAssertEqual(SidebarModel.groupLabel(host: "local", project: "api"), "api")
        XCTAssertEqual(SidebarModel.groupLabel(host: "", project: "api"), "api")
        XCTAssertEqual(SidebarModel.groupLabel(host: "arch", project: "api"), "arch / api")
    }

    func testTheSameProjectOnTwoMachinesStaysInTwoGroups() {
        var local = row(1, .working)
        local.group = "api"
        var remote = row(2, .idle)
        remote.group = "api"
        remote.host = "arch"

        let sections = SidebarModel.sections([local, remote])
        XCTAssertEqual(sections.map(\.host), ["local", "arch"])
        XCTAssertEqual(sections.map(\.group), ["api", "api"])
        XCTAssertEqual(sections.map(\.key), ["api", "arch / api"])
        XCTAssertEqual(sections[0].sessions.map(\.id), [1])
        XCTAssertEqual(sections[1].sessions.map(\.id), [2])
    }

    func testAMachinesGroupsStayTogetherEvenWhenSessionsInterleave() {
        // Sessions arrive host by host in practice, but a machine heading must appear once:
        // all of a machine's groups follow it, in first-seen order.
        var a = row(1, .idle); a.group = "api"
        var b = row(2, .idle); b.group = "web"; b.host = "arch"
        var c = row(3, .idle); c.group = "docs"
        let sections = SidebarModel.sections([a, b, c])
        XCTAssertEqual(sections.map(\.host), ["local", "local", "arch"])
        XCTAssertEqual(sections.map(\.group), ["api", "docs", "web"])
    }

    func testTheMachineHeadingNamesThisMacOrTheSshTarget() {
        XCTAssertEqual(SidebarModel.machineLabel(host: "local", localName: "Jeffrey's MacBook Air"),
                       "Jeffrey's MacBook Air")
        XCTAssertEqual(SidebarModel.machineLabel(host: "", localName: "mini"), "mini")
        XCTAssertEqual(SidebarModel.machineLabel(host: "local", localName: ""), "This Mac",
                       "a failed name lookup still gets a heading")
        XCTAssertEqual(SidebarModel.machineLabel(host: "arch", localName: "mini"), "arch")
        XCTAssertEqual(SidebarModel.machineLabel(host: "10.0.0.5", localName: "mini"), "10.0.0.5")
    }

    func testStatusSymbolsCoverEveryStateWithNoSymbolForIdle() {
        let symbols = SidebarSession.Status.allCases.map { status in
            SidebarModel.statusSymbol(for: row(1, status))
        }
        XCTAssertEqual(symbols, [
            "exclamationmark.circle",
            "exclamationmark.triangle.fill",
            "checkmark",
            "circle.dashed",
            nil,
        ])
        var unreadDone = row(2, .done)
        unreadDone.unread = 1
        XCTAssertEqual(SidebarModel.statusSymbol(for: unreadDone), "checkmark.circle.fill")
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
        state.renamedGroups["pigeon"] = "pigeon-core"
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

final class GitRootTests: XCTestCase {
    func testFindsTheNearestGitDirectory() throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-git-\(UUID().uuidString)")
        let nested = root.appendingPathComponent("core/src")
        try FileManager.default.createDirectory(at: nested, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(
            at: root.appendingPathComponent(".git"), withIntermediateDirectories: true)
        XCTAssertEqual(GitRoot.find(cwd: nested.path), root.path)
        XCTAssertNil(GitRoot.find(cwd: FileManager.default.temporaryDirectory.path))
        XCTAssertNil(GitRoot.find(cwd: ""))
        try? FileManager.default.removeItem(at: root)
    }
}
