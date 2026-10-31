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
        XCTAssertEqual(banner, "Connection lost. Reconnecting…")
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

    func testALostRemoteSessionIsRecoveredByReprobingSsh() {
        XCTAssertEqual(HostRecovery.plan(hasRemote: true), .remote)
        XCTAssertEqual(HostRecovery.plan(hasRemote: false), .local)
    }

    /// A network drop is not a crashed local socket: the remote probe has to keep trying for
    /// far longer than the local restart loop (the M4 acceptance is a 30 s outage).
    func testTheRemoteBudgetOutlastsTheLocalOne() {
        XCTAssertTrue(HostRecovery.canRetryRemote(after: 0))
        XCTAssertTrue(
            HostRecovery.canRetryRemote(after: HostRecovery.attempts),
            "the local budget is used up long before the remote one")
        XCTAssertTrue(HostRecovery.canRetryRemote(after: HostRecovery.remoteAttempts - 1))
        XCTAssertFalse(HostRecovery.canRetryRemote(after: HostRecovery.remoteAttempts))
        let window = Double(HostRecovery.remoteAttempts) * HostRecovery.remoteRetryDelay
        XCTAssertGreaterThanOrEqual(window, 30, "must outlast the 30 s outage it is for")
    }

    /// Help text reaches every user: no developer commands, no internal names.
    func testHelpTextSpeaksToUsersNotDevelopers() {
        let help = HostRecovery.remoteHelp(host: "example.com", reason: "Can't connect to example.com")
        XCTAssertTrue(help.contains("example.com with SSH"), help)
        for text in [help, HostRecovery.refusal(reason: "version", machine: "This Mac")] {
            for jargon in ["pigeon-host", "cargo", "`", "session host"] {
                XCTAssertFalse(text.contains(jargon), "\(jargon) in: \(text)")
            }
        }
    }

    func testARefusalSaysWhatToDo() {
        XCTAssertEqual(
            HostRecovery.refusal(reason: "version", machine: "box"),
            "box is running a different version of Pigeon. Update Pigeon, then click Retry.")
        XCTAssertEqual(
            HostRecovery.refusal(reason: "made-up", machine: "box"),
            "box didn't accept this connection.", "an unknown reason still reads as a sentence")
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

/// Issue #4: which panes ask the host for just the frames they missed.
final class ResumeTests: XCTestCase {
    func testAPaneStillOnScreenResumesFromItsLastFrame() {
        XCTAssertEqual(HostRecovery.resumeSeq(drawn: 41, stale: true, onScreen: true), 41)
    }

    func testEverythingElseGetsASnapshot() {
        XCTAssertNil(HostRecovery.resumeSeq(drawn: 41, stale: false, onScreen: true),
                     "not a reconnect: an ordinary attach")
        XCTAssertNil(HostRecovery.resumeSeq(drawn: 41, stale: true, onScreen: false),
                     "nothing drawn to patch")
        XCTAssertNil(HostRecovery.resumeSeq(drawn: nil, stale: true, onScreen: true),
                     "the host restarted; its numbers mean nothing here")
    }
}
