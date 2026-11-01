import XCTest
@testable import PigeonApp

/// What `RemoteSession` hands to ssh and to the remote shell: a destination that cannot be
/// read as an option, quoted paths, and the host started with `--detach`.
final class RemoteCommandTests: XCTestCase {
    /// A "host" that starts with `-` would reach ssh as an option (`-oProxyCommand=…` runs a
    /// local command). It is refused before anything runs, not degraded to plain ssh.
    func testAHostThatLooksLikeAnOptionIsRefused() {
        var ran = false
        var spawned = false
        let session = RemoteSession(
            host: "-oProxyCommand=touch /tmp/pwned",
            installer: HostInstaller(run: { _, _, _ in
                ran = true
                return HostInstaller.Result(status: 0, output: "", error: "")
            }),
            run: { _ in
                ran = true
                return HostInstaller.Result(status: 0, output: "", error: "")
            },
            spawn: { _ in
                spawned = true
                return Process()
            },
            artifact: { _ in nil }
        )
        XCTAssertThrowsError(try session.connect()) { error in
            XCTAssertTrue("\(error)".contains("not a valid SSH destination"), "got \(error)")
        }
        XCTAssertFalse(ran)
        XCTAssertFalse(spawned)
    }

    /// Every ssh call ends its options with `--`, so the destination can never be read as one.
    func testTheDestinationAlwaysFollowsAnEndOfOptionsMarker() throws {
        var calls: [[String]] = []
        let session = RemoteSession(
            host: "far",
            run: { _ in HostInstaller.Result(status: 0, output: "", error: "") },
            spawn: { arguments in
                calls.append(arguments)
                return Process()
            }
        )
        _ = try session.connect(allowHost: false)
        XCTAssertEqual(calls, [["-tt", "--", "far"]])
    }

    /// The installed path comes back from the remote, so it is quoted for the shell, never
    /// pasted inside double quotes where `$(…)` or a `"` would still be live.
    func testTheRemoteBinaryPathIsShellQuoted() throws {
        var scripts: [String] = []
        let session = RemoteSession(
            host: "far",
            run: { script in
                scripts.append(script)
                return HostInstaller.Result(status: 0, output: "/s.sock", error: "")
            },
            spawn: { _ in Process() }
        )
        _ = try session.remoteSocket(binary: "/home/a'b/$(id)/pigeon-server")
        XCTAssertEqual(scripts, ["'/home/a'\\''b/$(id)/pigeon-server' --socket-path"])
        XCTAssertEqual(HostInstaller.shellQuote("plain"), "'plain'")
        XCTAssertEqual(HostInstaller.shellQuote("it's"), "'it'\\''s'")
    }

    /// The remote host is started the way the local one is: `--detach` asks the socket, not
    /// the process table, whether a host is up, so another binary that merely has the same
    /// path in its arguments cannot pass for one, and a fresh host gets its own process group.
    func testTheRemoteHostIsStartedWithDetach() throws {
        var scripts: [String] = []
        let session = RemoteSession(
            host: "far",
            run: { script in
                scripts.append(script)
                return HostInstaller.Result(status: 0, output: "", error: "")
            },
            spawn: { _ in Process() }
        )
        try session.startDaemon(binary: "/home/u/.pigeon/server/v/pigeon-server")
        let script = try XCTUnwrap(scripts.first)
        XCTAssertTrue(script.contains("'/home/u/.pigeon/server/v/pigeon-server' --detach"), script)
        XCTAssertFalse(script.contains("pgrep"), script)
        XCTAssertFalse(script.contains("nohup"), script)
    }
}
