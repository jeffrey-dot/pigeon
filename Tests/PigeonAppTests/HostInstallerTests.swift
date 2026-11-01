import XCTest
@testable import PigeonApp

/// The remote installer's rules: which binary goes to which platform, what the remote shell
/// does, and that a runnable binary of the right version is used without any upload.
final class HostInstallerTests: XCTestCase {
    // MARK: - pure

    func testSshRunsShareOneMasterConnectionInAPrivateDirectory() throws {
        let options = HostInstaller.sshOptions()
        XCTAssertTrue(options.contains("BatchMode=yes"))
        XCTAssertTrue(options.contains("ControlMaster=auto"), "one handshake for all the probes")
        let path = try XCTUnwrap(options.first { $0.hasPrefix("ControlPath=") })
        XCTAssertEqual(path, "ControlPath=/tmp/pigeon-\(getuid())-ssh/%C")
        let attributes = try FileManager.default.attributesOfItem(atPath: "/tmp/pigeon-\(getuid())-ssh")
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o700)
    }

    func testAForwardAndAnUnsafeDirectoryDoNotMultiplex() throws {
        // `-L` through a master exits at once and leaves the forward in the master.
        XCTAssertFalse(HostInstaller.sshOptions(multiplex: false).contains("ControlMaster=auto"))
        // A directory this user does not own (or that others can enter) is never used.
        let shared = "/tmp/pigeon-\(getuid() &+ 7777)-ssh"
        try? FileManager.default.removeItem(atPath: shared)
        try FileManager.default.createDirectory(
            atPath: shared, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o777])
        defer { try? FileManager.default.removeItem(atPath: shared) }
        XCTAssertNil(HostInstaller.controlDirectory(uid: getuid() &+ 7777), "owned by someone else")
        let loose = try XCTUnwrap(HostInstaller.controlDirectory(uid: getuid()), "made on first use")
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: loose)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: loose) }
        XCTAssertNil(HostInstaller.controlDirectory(uid: getuid()), "readable by others")
    }

    func testOnlyAMatchingPlatformCanBeServed() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let linux = directory.appendingPathComponent("pigeon-server-x86_64-unknown-linux-musl")
        FileManager.default.createFile(atPath: linux.path, contents: Data("elf".utf8))
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: linux.path)

        XCTAssertEqual(HostInstaller.artifact(for: "Linux x86_64", in: [directory])?.path, linux.path)
        XCTAssertNil(
            HostInstaller.artifact(for: "Linux aarch64", in: [directory]),
            "a Linux x86_64 binary must never be pushed to another platform")
        XCTAssertNil(HostInstaller.artifact(for: "Darwin arm64", in: [directory]))
        XCTAssertNil(HostInstaller.artifact(for: "Linux riscv64", in: [directory]))
    }

    func testACrossBuildDirectoryUsesThePlainName() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent("aarch64-unknown-linux-musl")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let plain = directory.appendingPathComponent("pigeon-server")
        FileManager.default.createFile(atPath: plain.path, contents: Data("elf".utf8))
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: plain.path)

        XCTAssertEqual(HostInstaller.artifact(for: "Linux aarch64", in: [directory])?.path, plain.path)
    }

    func testTheRemoteScriptRunsTheUploadBeforeItGoesLive() {
        let script = HostInstaller.installScript(
            directory: "$HOME/.pigeon/server/0.1.0", expected: "pigeon-server 0.1.0 proto 1.0")
        XCTAssertTrue(script.contains("$HOME/.pigeon/server/0.1.0/pigeon-server.tmp"))
        XCTAssertTrue(script.contains("\"$tmp\" --version"), "the upload has to run here")
        XCTAssertTrue(script.contains("if [ \"$got\" != \"pigeon-server 0.1.0 proto 1.0\" ]"))
        XCTAssertTrue(script.contains("rm -f \"$tmp\""), "a bad transfer must not be left behind")
        XCTAssertTrue(
            script.contains("mv \"$tmp\" \"$HOME/.pigeon/server/0.1.0/pigeon-server\""),
            "the binary only appears after it runs")
        XCTAssertTrue(script.contains("tail -n +3"), "older versions are garbage collected")
        XCTAssertFalse(script.contains("/tmp/"), "nothing is written outside ~/.pigeon")
        XCTAssertFalse(script.contains("sha256"), "no hashing on the remote")
    }

    func testTheProbeAsksEveryInstalledBinaryItsVersion() {
        let script = HostInstaller.probeScript
        XCTAssertTrue(script.contains("ls -1t"), "newest first")
        XCTAssertTrue(script.contains("[ -x \"$bin\" ]"), "a non-executable file is skipped")
        XCTAssertTrue(script.contains("\"$bin\" --version"))
        XCTAssertFalse(script.contains("sha256"))
    }

    func testVersionLinesAreReadWholeOrNotAtAll() {
        XCTAssertEqual(HostInstaller.versionLine("0.1.0", proto: 0x0103), "pigeon-server 0.1.0 proto 1.3")
        let parsed = HostInstaller.parseVersionLine("pigeon-server 0.2.0 proto 1.3")
        XCTAssertEqual(parsed?.version, "0.2.0")
        XCTAssertEqual(parsed?.proto, 0x0103)
        for bad in [
            "", "pigeon-server 0.1.0", "pigeon-server 0.1.0 proto", "pigeon-server 0.1.0 proto 1",
            "pigeon-server 0.1.0 proto x.0", "pigeon-server 0.1.0 proto 1.0 extra",
            "other 0.1.0 proto 1.0", "pigeon-server 0.1.0 proto 256.0",
        ] {
            XCTAssertNil(HostInstaller.parseVersionLine(bad), bad)
        }
    }

    /// Release versions do not matter; the protocol major does. Minor differences are the
    /// handshake's business, so they are no reason to upload.
    func testTheNewestServerWithTheSameProtocolMajorIsChosen() {
        let listing = """
            /h/.pigeon/server/0.3.0/pigeon-server\tpigeon-server 0.3.0 proto 2.0
            /h/.pigeon/server/0.2.5/pigeon-server\t
            /h/.pigeon/server/0.2.0/pigeon-server\tpigeon-server 0.2.0 proto 1.4
            /h/.pigeon/server/0.1.0/pigeon-server\tpigeon-server 0.1.0 proto 1.0
            /h/.pigeon/server/0123456789abcdef/pigeon-server\tpigeon-server 0.1.0

            """
        let found = HostInstaller.compatible(in: listing, proto: 0x0100)
        XCTAssertEqual(found?.path, "/h/.pigeon/server/0.2.0/pigeon-server", "newer minor is fine")
        XCTAssertEqual(found?.version, "0.2.0")
        XCTAssertEqual(HostInstaller.compatible(in: listing, proto: 0x0201)?.version, "0.3.0")
        XCTAssertNil(HostInstaller.compatible(in: listing, proto: 0x0300), "no major 3 installed")
        XCTAssertNil(HostInstaller.compatible(in: "", proto: 0x0100))
    }

    func testTheExpectedVersionIsTheCoresOwn() {
        XCTAssertFalse(HostInstaller.expectedVersion.isEmpty)
        XCTAssertTrue(HostInstaller.expectedVersion.first?.isNumber ?? false, HostInstaller.expectedVersion)
        XCTAssertEqual(HostInstaller.protocolVersion >> 8, 1)
    }

    /// A compatible server is used as is even when the app is newer: no local file is looked
    /// for, nothing is downloaded, nothing is sent.
    func testACompatibleServerIsUsedAsIs() throws {
        var uploads = 0
        let installer = HostInstaller(run: { _, script, stdin in
            if script.contains("uname -sm") {
                return HostInstaller.Result(status: 0, output: "Linux x86_64\n", error: "")
            }
            XCTAssertNil(stdin)
            if script.contains("set -e") { uploads += 1 }
            return HostInstaller.Result(
                status: 0,
                output: "/home/u/.pigeon/server/0.1.0/pigeon-server\tpigeon-server 0.1.0 proto 1.0\n",
                error: "")
        }, version: "0.2.0", proto: 0x0101)
        let remote = try installer.install(host: "far") { _ in
            XCTFail("no local binary is needed")
            throw HostInstaller.Failure.noArtifact("Linux x86_64")
        }
        XCTAssertFalse(remote.sent)
        XCTAssertEqual(remote.path, "/home/u/.pigeon/server/0.1.0/pigeon-server")
        XCTAssertEqual(remote.version, "0.1.0")
        XCTAssertEqual(uploads, 0)
    }

    /// Nothing installed, nothing that runs here, or only another protocol major: upload.
    func testAnIncompatibleOrMissingServerIsReplaced() throws {
        let artifact = try fakeArtifact()
        defer { try? FileManager.default.removeItem(at: artifact) }
        for listing in [
            "",
            "/p/pigeon-server\t\n",
            "/p/pigeon-server\tpigeon-server 9.0.0 proto 2.0\n",
            "/p/pigeon-server\tpigeon-server 0.1.0\n",
        ] {
            var sent: URL?
            var script = ""
            let installer = HostInstaller(run: { _, body, stdin in
                if body.contains("uname -sm") {
                    return HostInstaller.Result(status: 0, output: "Linux aarch64", error: "")
                }
                if body.contains("set -e") {
                    sent = stdin
                    script = body
                    return HostInstaller.Result(
                        status: 0, output: "/home/u/.pigeon/server/0.2.0/pigeon-server", error: "")
                }
                return HostInstaller.Result(status: 0, output: listing, error: "")
            }, version: "0.2.0", proto: 0x0100)
            var asked: String?
            let remote = try installer.install(host: "far") { platform in
                asked = platform
                return artifact
            }
            XCTAssertTrue(remote.sent, listing)
            XCTAssertEqual(remote.version, "0.2.0")
            XCTAssertEqual(asked, "Linux aarch64", "the local build is for the remote's platform")
            XCTAssertEqual(sent, artifact, listing)
            XCTAssertTrue(script.contains("$HOME/.pigeon/server/0.2.0"), "goes in under the app's version")
            XCTAssertTrue(script.contains("pigeon-server 0.2.0 proto 1.0"))
        }
    }

    func testInstallsCanBeRefused() throws {
        let installer = HostInstaller(run: { _, script, _ in
            if script.contains("uname -sm") {
                return HostInstaller.Result(status: 0, output: "Linux x86_64", error: "")
            }
            return HostInstaller.Result(status: 0, output: "", error: "")
        }, version: "0.2.0")
        XCTAssertThrowsError(try installer.install(host: "far", allowInstall: false) { _ in
            XCTFail("refused installs do not fetch")
            throw HostInstaller.Failure.noArtifact("")
        }) {
            XCTAssertTrue("\($0)".contains("refused"), "got \($0)")
        }
    }

    func testAnUnsupportedPlatformIsReportedNotGuessed() throws {
        let installer = HostInstaller(run: { _, _, _ in
            HostInstaller.Result(status: 0, output: "SunOS sparc", error: "")
        })
        XCTAssertThrowsError(try installer.install(host: "ancient") { _ in URL(fileURLWithPath: "/") }) {
            XCTAssertTrue("\($0)".contains("SunOS sparc"), "got \($0)")
        }
    }

    func testAnUploadThatDoesNotRunIsAFailureWithTheRemoteReason() throws {
        let artifact = try fakeArtifact()
        defer { try? FileManager.default.removeItem(at: artifact) }
        let installer = HostInstaller(run: { _, script, _ in
            if script.contains("uname -sm") {
                return HostInstaller.Result(status: 0, output: "Linux x86_64", error: "")
            }
            if script.contains("set -e") {
                return HostInstaller.Result(
                    status: 3, output: "", error: "pigeon-server: the uploaded file does not run here (got '')")
            }
            return HostInstaller.Result(status: 0, output: "", error: "")
        })
        XCTAssertThrowsError(try installer.install(host: "far") { _ in artifact }) {
            XCTAssertTrue("\($0)".contains("does not run here"), "got \($0)")
        }
    }

    // MARK: - against a real ssh host (PIGEON_SSH_HOST=arch)

    /// The whole point of the installer is that it works on a machine we do not control, so
    /// the end-to-end check is a real ssh session. Opt-in: `PIGEON_SSH_HOST=arch swift test`.
    func testRealRemoteInstallIsCachedVerifiedAndConfined() throws {
        guard let host = ProcessInfo.processInfo.environment["PIGEON_SSH_HOST"], !host.isEmpty else {
            throw XCTSkip("set PIGEON_SSH_HOST to run the remote install check")
        }
        let platform = try HostInstaller().platform(of: host)
        guard let artifact = HostInstaller.artifact(
            for: platform, in: HostInstaller.searchDirectories())
        else {
            throw XCTSkip("no local binary for \(platform); build one with `cargo build --release --target x86_64-unknown-linux-musl`")
        }
        // The local build reports the crate version; a throwaway directory name keeps "first
        // connection uploads" true on every run without touching the real install.
        let version = HostInstaller.expectedVersion
        let scratch = "$HOME/.pigeon/server/e2e-\(UUID().uuidString.prefix(8))"
        defer {
            _ = try? HostInstaller.sshRunner(host: host, script: "rm -rf \"\(scratch)\"", stdin: nil)
        }
        let line = HostInstaller.versionLine(version, proto: HostInstaller.protocolVersion)
        let install = HostInstaller.installScript(directory: scratch, expected: line)
        let sent = try HostInstaller.sshRunner(host: host, script: install, stdin: artifact)
        XCTAssertTrue(sent.ok, "upload passes the remote check: \(sent.error)")

        let probe = try HostInstaller.sshRunner(host: host, script: HostInstaller.probeScript, stdin: nil)
        XCTAssertTrue(probe.output.contains(line), probe.output)

        // Something that does not run there must leave nothing behind.
        let junk = FileManager.default.temporaryDirectory.appendingPathComponent("pigeon-junk-\(UUID().uuidString)")
        try Data("not a binary".utf8).write(to: junk)
        defer { try? FileManager.default.removeItem(at: junk) }
        let bad = "$HOME/.pigeon/server/e2e-bad-\(UUID().uuidString.prefix(8))"
        let rejected = try HostInstaller.sshRunner(
            host: host,
            script: HostInstaller.installScript(directory: bad, expected: line),
            stdin: junk)
        XCTAssertFalse(rejected.ok)
        let leftovers = try HostInstaller.sshRunner(
            host: host, script: "ls -d \"\(bad)\" 2>/dev/null | wc -l", stdin: nil)
        XCTAssertEqual(leftovers.output.trimmingCharacters(in: .whitespacesAndNewlines), "0")
    }

    // MARK: - helpers

    private func temporaryDirectory() throws -> URL {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-installer-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    private func fakeArtifact() throws -> URL {
        let directory = try temporaryDirectory()
        let file = directory.appendingPathComponent("pigeon-server-x86_64-unknown-linux-musl")
        FileManager.default.createFile(atPath: file.path, contents: Data("not really an elf".utf8))
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
        return file
    }
}
