import AppKit
import XCTest
@testable import PigeonApp

final class SidebarLayoutTests: XCTestCase {
    private func descendants(of view: NSView) -> [NSView] {
        view.subviews.flatMap { [$0] + descendants(of: $0) }
    }

    func testLongTaskTextKeepsOneRowAndRemainsAvailableInTheTooltip() throws {
        let cell = SidebarRowCellView(frame: NSRect(x: 0, y: 0, width: 200, height: 30))
        let question = String(repeating: "Review the database migration. ", count: 12)
        let session = SidebarSession(
            id: 1, title: "a-very-long-session-title-that-will-truncate", group: "api",
            status: .blocked, reported: true,
            agent: AgentInfo(app: "claude", message: question, kind: "permission"))
        cell.configure(with: session)
        cell.layoutSubtreeIfNeeded()
        let title = try XCTUnwrap(cell.subviews.first { $0.identifier?.rawValue == "session.title" })
        let age = try XCTUnwrap(cell.subviews.first { $0.identifier?.rawValue == "session.age" })
        let status = try XCTUnwrap(cell.subviews.first { $0.identifier?.rawValue == "session.status" })
        XCTAssertEqual(title.frame.midY, age.frame.midY, accuracy: 1)
        XCTAssertEqual(age.frame.midY, status.frame.midY, accuracy: 1)
        XCTAssertLessThanOrEqual(title.frame.maxX, age.frame.minX)
        XCTAssertLessThan(age.frame.maxX, status.frame.minX)
        XCTAssertLessThanOrEqual(status.frame.maxX, cell.bounds.maxX)
        XCTAssertTrue(cell.toolTip?.contains(question.trimmingCharacters(in: .whitespacesAndNewlines)) == true)
        XCTAssertEqual(cell.subviews.compactMap { $0 as? NSTextField }.count, 2)
    }

    func testUnreportedShellHasNoStatusAndMultiClientSizeRemainsVisible() throws {
        let cell = SidebarRowCellView(frame: NSRect(x: 0, y: 0, width: 272, height: 30))
        cell.configure(with: SidebarSession(
            id: 1, title: "shell", group: "api", status: .idle,
            sizeSource: "locked", clients: 2))
        cell.layoutSubtreeIfNeeded()
        let age = try XCTUnwrap(cell.subviews.compactMap { $0 as? NSTextField }
            .first { $0.identifier?.rawValue == "session.age" })
        let status = try XCTUnwrap(cell.subviews.first { $0.identifier?.rawValue == "session.status" })
        XCTAssertTrue(status.isHidden)
        XCTAssertEqual(age.stringValue, "2× locked")
        XCTAssertGreaterThanOrEqual(age.frame.width, age.intrinsicContentSize.width - 1)
    }

    func testLocalProjectsStartImmediatelyAndRemoteHostsKeepTheirIdentity() throws {
        let controller = SidebarViewController()
        controller.loadView()
        controller.view.frame = NSRect(x: 0, y: 0, width: 272, height: 600)
        let local = SidebarSession(
            id: 1, title: "local", group: "api", status: .blocked,
            agent: AgentInfo(message: "A long permission question"))
        let localSection = SidebarSection(host: "local", key: "api", group: "api", sessions: [local])
        controller.update(sections: [localSection], selected: 1) { $0 }
        let table = try XCTUnwrap(descendants(of: controller.view).compactMap { $0 as? NSTableView }.first)
        XCTAssertEqual(table.numberOfRows, 2, "one project and one session, without a local machine heading")
        XCTAssertEqual(controller.tableView(table, heightOfRow: 1), 30)
        XCTAssertTrue(controller.selectRow(id: 1))

        let remote = SidebarSession(id: 2, title: "remote", group: "api", status: .idle, host: "devbox")
        controller.update(sections: [localSection,
            SidebarSection(host: "devbox", key: "devbox / api", group: "api", sessions: [remote])
        ], selected: 2) { $0 }
        XCTAssertEqual(table.numberOfRows, 6, "each machine retains its own heading and project")
        var selected: Int?
        controller.onSelect = { selected = $0 }
        XCTAssertTrue(controller.selectRow(id: 2))
        XCTAssertEqual(selected, 2)
    }

    func testHeaderButtonsStillDispatchNewSessionAndSidebarToggle() throws {
        let controller = SidebarViewController()
        controller.loadView()
        var created = false
        var toggled = false
        controller.onNewSession = { _ in created = true }
        controller.onToggleSidebar = { toggled = true }
        let buttons = descendants(of: controller.view).compactMap { $0 as? NSButton }
        let newSession = try XCTUnwrap(buttons.first { $0.action?.description == "newSessionClicked" })
        let toggle = try XCTUnwrap(buttons.first { $0.action?.description == "toggleSidebarClicked" })
        newSession.performClick(nil)
        toggle.performClick(nil)
        XCTAssertTrue(created)
        XCTAssertTrue(toggled)
    }

    func testTheNativeTitlebarDoesNotCoverSidebarButtonHitTargets() throws {
        _ = NSApplication.shared
        let delegate = AppDelegate()
        delegate.buildWindow()
        delegate.window.isReleasedWhenClosed = false
        defer { delegate.window.close() }
        let frameView = try XCTUnwrap(delegate.window.contentView?.superview)
        frameView.layoutSubtreeIfNeeded()
        let buttons = descendants(of: delegate.sidebar.view).compactMap { $0 as? NSButton }
        for action in ["newSessionClicked", "toggleSidebarClicked"] {
            let button = try XCTUnwrap(buttons.first { $0.action?.description == action })
            let point = button.convert(NSPoint(x: button.bounds.midX, y: button.bounds.midY), to: frameView)
            let hit = frameView.hitTest(point)
            XCTAssertTrue(hit === button || hit?.isDescendant(of: button) == true,
                          "\(action) is covered by \(String(describing: hit))")
        }
        delegate.toggleSidebar(nil)
        XCTAssertTrue(delegate.sidebar.view.isHidden)
        delegate.toggleSidebar(nil)
        frameView.layoutSubtreeIfNeeded()
        XCTAssertFalse(delegate.sidebar.view.isHidden)
        XCTAssertGreaterThan(delegate.sidebar.view.frame.width, 0)
    }
}
