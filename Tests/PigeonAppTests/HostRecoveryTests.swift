import Foundation
import XCTest
@testable import PigeonApp

/// The window has to explain a dead host and keep trying on its own; these are the rules
/// behind that banner, separated from AppKit so they can be checked.
final class HostRecoveryTests: XCTestCase {
    func testConnectedShowsNothing() {
        XCTAssertNil(HostRecovery.banner(for: .connected))
        XCTAssertNil(HostRecovery.action(for: .connected), "nothing to press while connected")
    }

    func testReconnectingNamesTheAttemptButOffersNoButton() {
        let state = HostRecovery.State.reconnecting(attempt: 2)
        let banner = HostRecovery.banner(for: state)
        XCTAssertEqual(banner, "Session host is not reachable — reconnecting (2/3)…")
        XCTAssertNil(HostRecovery.action(for: state), "a retry is already in flight")
    }

    func testStoppedKeepsTheReasonAndOffersARetry() {
        let state = HostRecovery.State.stopped(detail: "pigeon-host: permission denied")
        XCTAssertEqual(HostRecovery.banner(for: state), "pigeon-host: permission denied")
        XCTAssertEqual(HostRecovery.action(for: state), "Retry")
    }

    func testItGivesUpAfterTheLastAttempt() {
        XCTAssertTrue(HostRecovery.canRetry(after: 0))
        XCTAssertTrue(HostRecovery.canRetry(after: HostRecovery.attempts - 1))
        XCTAssertFalse(HostRecovery.canRetry(after: HostRecovery.attempts))
        XCTAssertFalse(HostRecovery.canRetry(after: HostRecovery.attempts + 1))
    }

    func testTheHelpTextNamesTheRealCommand() {
        XCTAssertTrue(HostRecovery.startCommand.contains("--bin pigeon-host"))
    }

    func testLauncherPrefersTheExplicitPathThenTheWorkingDirectory() {
        let candidates = HostLauncher.candidates(
            environment: ["PIGEON_HOST_PATH": "/opt/pigeon/host"],
            executable: "/Applications/Pigeon.app/Contents/MacOS/pigeon",
            workingDirectory: "/repo"
        )
        XCTAssertEqual(candidates.first, "/opt/pigeon/host")
        XCTAssertEqual(candidates.dropFirst().first, "/Applications/Pigeon.app/Contents/MacOS/pigeon-host")
        XCTAssertTrue(candidates.contains("/repo/core/target/debug/pigeon-host"))
    }

    /// A killed host must reach the window as a closed socket, not as silence: this is the
    /// signal the banner and the retry loop hang off.
    func testAClosedHostSocketIsReported() throws {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-lost-\(UUID().uuidString).sock").path
        let listener = socket(AF_UNIX, SOCK_STREAM, 0)
        XCTAssertGreaterThanOrEqual(listener, 0)
        defer { Darwin.close(listener); try? FileManager.default.removeItem(atPath: path) }

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: Array(path.utf8))
        }
        let size = socklen_t(MemoryLayout<sockaddr_un>.size)
        let bound = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.bind(listener, $0, size)
            }
        }
        XCTAssertEqual(bound, 0)
        XCTAssertEqual(Darwin.listen(listener, 1), 0)

        let client = try HostClient.connect(path: path)
        let closed = expectation(description: "client noticed the host dying")
        client.onClose = { closed.fulfill() }

        let peer = accept(listener, nil, nil)
        XCTAssertGreaterThanOrEqual(peer, 0)
        // `kill -9` looks like this from the client side: the peer simply goes away.
        Darwin.close(peer)

        wait(for: [closed], timeout: 5)
        client.close()
    }

    /// The retry loop only helps if the app can really start the host; run a throwaway
    /// executable and wait for the file it writes.
    func testLaunchStartsARealProcess() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-spawn-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let marker = directory.appendingPathComponent("started").path
        let script = directory.appendingPathComponent("pigeon-host")
        try "#!/bin/sh\ntouch \(marker)\n".write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755], ofItemAtPath: script.path)

        XCTAssertTrue(HostLauncher.launch(path: script.path))
        let deadline = Date().addingTimeInterval(5)
        while !FileManager.default.fileExists(atPath: marker), Date() < deadline {
            usleep(50_000)
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: marker), "the process actually ran")
    }

    func testFindSkipsMissingBinaries() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-launch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let missing = directory.appendingPathComponent("missing").path
        let found = directory.appendingPathComponent("pigeon-host")
        FileManager.default.createFile(atPath: found.path, contents: Data("#!/bin/sh\n".utf8))
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755], ofItemAtPath: found.path)

        XCTAssertNil(HostLauncher.find(candidates: [missing]))
        XCTAssertEqual(HostLauncher.find(candidates: [missing, found.path]), found.path)
    }
}
