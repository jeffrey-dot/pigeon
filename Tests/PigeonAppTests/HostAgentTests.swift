import Foundation
import XCTest
@testable import PigeonApp

/// Moving off the launchd job older builds registered. launchctl is faked: these check what
/// the app asks for, never the user's real `dev.pigeon.host`.
final class HostAgentTests: XCTestCase {
    private var directory: URL!
    private var calls: [[String]] = []

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-agent-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        calls = []
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: directory)
    }

    private func agent(print output: String) -> HostAgent {
        HostAgent(
            label: "dev.pigeon.test",
            plistURL: directory.appendingPathComponent("dev.pigeon.test.plist"),
            uid: 501,
            launchctl: { [unowned self] arguments in
                calls.append(arguments)
                return (0, arguments.first == "print" ? output : "")
            })
    }

    private func installPlist(_ agent: HostAgent) throws {
        try Data("<plist/>".utf8).write(to: agent.plistURL)
    }

    func testNothingInstalledMeansNothingToDo() {
        let agent = agent(print: "")
        XCTAssertFalse(agent.removeLegacyJob())
        XCTAssertTrue(calls.isEmpty, "no launchctl at all on a Mac that never had the job")
    }

    func testAnIdleJobIsUnloadedAndForgotten() throws {
        let agent = agent(print: "state = spawn scheduled\n")
        try installPlist(agent)
        XCTAssertTrue(agent.removeLegacyJob())
        XCTAssertEqual(calls, [["print", "gui/501/dev.pigeon.test"], ["bootout", "gui/501/dev.pigeon.test"]])
        XCTAssertFalse(agent.isInstalled)
    }

    func testARunningJobKeepsItsSessionsAndOnlyLosesThePlist() throws {
        let agent = agent(print: "\tstate = running\n\tpid = 4242\n")
        try installPlist(agent)
        XCTAssertTrue(agent.removeLegacyJob())
        XCTAssertFalse(calls.contains { $0.first == "bootout" }, "bootout would end every session")
        XCTAssertFalse(agent.isInstalled, "gone from login items and the next login")
    }

    func testPidParsingRejectsJunk() {
        XCTAssertEqual(HostAgent.pid(inPrint: "a = b\n\tpid = 17\n"), 17)
        XCTAssertNil(HostAgent.pid(inPrint: "pid = \n"))
        XCTAssertNil(HostAgent.pid(inPrint: "pid = -3\n"))
        XCTAssertNil(HostAgent.pid(inPrint: "ppid = 9\npidfile = x\n"))
    }

    func testTheHostStartsDetachedWithItsLog() {
        XCTAssertEqual(
            HostLauncher.detachArguments(socket: "/s", shell: "/bin/zsh", log: "/l"),
            ["--detach", "--socket", "/s", "--log", "/l", "--shell", "/bin/zsh"])
        XCTAssertEqual(
            HostLauncher.detachArguments(socket: "/s", shell: "", log: "/l"),
            ["--detach", "--socket", "/s", "--log", "/l"])
    }
}
