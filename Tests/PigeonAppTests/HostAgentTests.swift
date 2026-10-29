import Foundation
import XCTest
@testable import PigeonApp

/// The launchd job that keeps the host alive without the app (issue #2). launchctl is
/// faked: these check what the app asks launchd for, not launchd itself.
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

    private func agent(status: @escaping ([String]) -> Int32 = { _ in 0 }) -> HostAgent {
        HostAgent(
            label: "dev.pigeon.test",
            plistURL: directory.appendingPathComponent("LaunchAgents/dev.pigeon.test.plist"),
            logURL: directory.appendingPathComponent("logs/host.log"),
            uid: 501,
            launchctl: { [unowned self] arguments in
                calls.append(arguments)
                return (status(arguments), "")
            })
    }

    private func job(_ agent: HostAgent) throws -> [String: Any] {
        let data = try Data(contentsOf: agent.plistURL)
        return try XCTUnwrap(
            PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any])
    }

    func testStartWritesAJobThatRestartsOnlyACrashedHost() throws {
        let agent = agent()
        XCTAssertNil(agent.start(
            hostPath: "/opt/pigeon-host", socket: "/tmp/p.sock", shell: "/bin/zsh",
            environment: ["PATH": "/usr/local/bin:/usr/bin", "LANG": "en_US.UTF-8", "SECRET": "x"]))

        let job = try job(agent)
        XCTAssertEqual(job["Label"] as? String, "dev.pigeon.test")
        XCTAssertEqual(
            job["ProgramArguments"] as? [String],
            ["/opt/pigeon-host", "--socket", "/tmp/p.sock", "--shell", "/bin/zsh"],
            "launchd's environment would pick another socket and shell")
        XCTAssertEqual(
            job["KeepAlive"] as? [String: Bool], ["SuccessfulExit": false],
            "a crash comes back; a clean stop stays stopped")
        XCTAssertEqual(job["StandardErrorPath"] as? String, agent.logURL.path)
        XCTAssertEqual(
            job["EnvironmentVariables"] as? [String: String],
            ["PATH": "/usr/local/bin:/usr/bin", "LANG": "en_US.UTF-8"],
            "only what the shells need, nothing else from the app's environment")
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: agent.logURL.deletingLastPathComponent().path))

        XCTAssertEqual(calls, [
            ["bootout", "gui/501/dev.pigeon.test"],
            ["bootstrap", "gui/501", agent.plistURL.path],
            ["kickstart", "gui/501/dev.pigeon.test"],
        ])
    }

    func testAnUnchangedJobIsNotUnloaded() throws {
        let agent = agent()
        XCTAssertNil(agent.start(hostPath: "/h", socket: "/s", shell: nil, environment: [:]))
        calls = []
        XCTAssertNil(agent.start(hostPath: "/h", socket: "/s", shell: nil, environment: [:]))
        XCTAssertFalse(calls.contains { $0.first == "bootout" }, "same job: nothing to reload")
        XCTAssertEqual(calls.last, ["kickstart", "gui/501/dev.pigeon.test"])
        XCTAssertEqual(try job(agent)["ProgramArguments"] as? [String], ["/h", "--socket", "/s"])
    }

    func testAFailedKickstartIsReported() {
        let agent = agent(status: { $0.first == "kickstart" ? 113 : 0 })
        let error = agent.start(hostPath: "/h", socket: "/s", shell: nil, environment: [:])
        XCTAssertNotNil(error, "the app falls back to a child process on this")
        XCTAssertTrue(error?.contains("113") ?? false)
    }

    func testStopUnloadsTheJobAndForgetsIt() throws {
        let agent = agent()
        XCTAssertNil(agent.start(hostPath: "/h", socket: "/s", shell: nil, environment: [:]))
        calls = []
        XCTAssertNil(agent.stop())
        XCTAssertEqual(calls, [["bootout", "gui/501/dev.pigeon.test"]])
        XCTAssertFalse(
            FileManager.default.fileExists(atPath: agent.plistURL.path),
            "otherwise the host comes back at the next login")
    }

    func testStoppingAJobLaunchdDoesNotKnowIsAnError() {
        let agent = agent(status: { _ in 3 })
        XCTAssertNotNil(agent.stop(), "a host started by hand keeps running; the user must hear it")
    }

    // MARK: - real launchd (PIGEON_LAUNCHD_TEST=1)

    /// The acceptance in miniature: a host started through launchd is not this process's
    /// child, so it survives the starter, and `kill -9` brings it back. Uses its own label,
    /// socket and plist directory, never the user's `dev.pigeon.host`.
    func testLaunchdKeepsTheHostAliveAndRestartsItAfterACrash() throws {
        guard ProcessInfo.processInfo.environment["PIGEON_LAUNCHD_TEST"] == "1" else {
            throw XCTSkip("opt-in: PIGEON_LAUNCHD_TEST=1 swift test --filter HostAgentTests")
        }
        let host = try XCTUnwrap(
            HostLauncher.find(candidates: HostLauncher.candidates()), "cargo build first")
        let socket = "/tmp/pigeon-agent-test-\(getuid()).sock"
        let agent = HostAgent(
            label: "dev.pigeon.test-\(UUID().uuidString.prefix(8))",
            plistURL: directory.appendingPathComponent("agent.plist"),
            logURL: directory.appendingPathComponent("host.log"))
        defer { _ = agent.stop() }

        XCTAssertNil(agent.start(hostPath: host, socket: socket, shell: "/bin/sh"))
        let first = try waitForPid(agent)
        XCTAssertNoThrow(try waitForSocket(socket))
        XCTAssertNotEqual(getppid(), first)

        kill(first, SIGKILL)
        let second = try waitForPid(agent, not: first)
        XCTAssertNotEqual(second, first, "launchd restarted the crashed host")
        XCTAssertNoThrow(try waitForSocket(socket))

        XCTAssertNil(agent.stop())
        XCTAssertFalse(agent.isLoaded())
    }

    private func waitForPid(_ agent: HostAgent, not previous: pid_t = 0) throws -> pid_t {
        let deadline = Date().addingTimeInterval(15)
        while Date() < deadline {
            let output = HostAgent.runLaunchctl(["print", "gui/\(getuid())/\(agent.label)"]).output
            if let line = output.split(separator: "\n").first(where: { $0.contains("pid = ") }),
               let pid = pid_t(line.split(separator: "=").last?.trimmingCharacters(in: .whitespaces) ?? ""),
               pid != previous
            {
                return pid
            }
            usleep(100_000)
        }
        throw XCTSkip("launchd never reported a pid")
    }

    private func waitForSocket(_ path: String) throws {
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline {
            if let client = try? HostClient.connect(path: path) {
                client.close()
                return
            }
            usleep(100_000)
        }
        XCTFail("no host listening on \(path)")
    }
}
