import XCTest
@testable import PigeonApp

/// The remote installer's rules: which binary goes to which platform, what the remote shell
/// does, and whether a second connection really skips the upload.
final class HostInstallerTests: XCTestCase {
    // MARK: - pure

    func testOnlyAMatchingPlatformCanBeServed() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let linux = directory.appendingPathComponent("pigeon-host-x86_64-unknown-linux-musl")
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
        let plain = directory.appendingPathComponent("pigeon-host")
        FileManager.default.createFile(atPath: plain.path, contents: Data("elf".utf8))
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: plain.path)

        XCTAssertEqual(HostInstaller.artifact(for: "Linux aarch64", in: [directory])?.path, plain.path)
    }

    func testTheRemoteScriptVerifiesBeforeItGoesLive() {
        let script = HostInstaller.installScript(
            directory: "$HOME/.pigeon/host/0123456789abcdef", expected: "abc123")
        XCTAssertTrue(script.contains("$HOME/.pigeon/host/0123456789abcdef/pigeon-host.tmp"))
        XCTAssertTrue(script.contains("if [ \"$got\" != \"abc123\" ]"))
        XCTAssertTrue(script.contains("rm -f \"$tmp\""), "a bad transfer must not be left behind")
        XCTAssertTrue(
            script.contains("mv \"$tmp\" \"$HOME/.pigeon/host/0123456789abcdef/pigeon-host\""),
            "the binary only appears after the hash matches")
        XCTAssertTrue(script.contains("tail -n +3"), "older versions are garbage collected")
        XCTAssertFalse(script.contains("/tmp/"), "nothing is written outside ~/.pigeon")
    }

    func testTheHashProbeIsSilentWhenTheFileIsMissing() {
        let script = HostInstaller.remoteHash("$HOME/.pigeon/host/v/pigeon-host")
        XCTAssertTrue(script.contains("[ -f "))
        XCTAssertTrue(script.contains("sha256sum"))
        XCTAssertTrue(script.contains("shasum -a 256"), "macOS remotes have no sha256sum")
        XCTAssertEqual(HostInstaller.firstWord("deadbeef  /path/to/file\n"), "deadbeef")
        XCTAssertEqual(HostInstaller.firstWord(""), "")
    }

    func testContentHashIsLowercaseHexSha256() throws {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-hash-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: file) }
        try Data().write(to: file)
        // The well-known SHA-256 of the empty string.
        XCTAssertEqual(
            try HostInstaller.contentHash(file),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    }

    func testASecondInstallReusesTheRemoteCopy() throws {
        let artifact = try fakeArtifact()
        defer { try? FileManager.default.removeItem(at: artifact) }
        let full = try HostInstaller.contentHash(artifact)
        let version = String(full.prefix(16))
        var uploads = 0
        var scripts: [String] = []
        let remotePath = "$HOME/.pigeon/host/\(version)/pigeon-host"
        let installer = HostInstaller(run: { _, script, stdin in
            scripts.append(script)
            if script.contains("uname -sm") {
                return HostInstaller.Result(status: 0, output: "Linux x86_64\n", error: "")
            }
            if script.contains("set -e") {
                XCTAssertNotNil(stdin, "the binary is streamed on stdin")
                uploads += 1
                return HostInstaller.Result(status: 0, output: remotePath, error: "")
            }
            if script.contains("printf '%s'") {
                return HostInstaller.Result(status: 0, output: remotePath, error: "")
            }
            // The hash probe: only the second call finds anything, and it matches.
            return HostInstaller.Result(
                status: 0, output: uploads == 0 ? "" : "\(full)  \(remotePath)\n", error: "")
        })

        let first = try installer.install(host: "arch", artifact: artifact)
        XCTAssertTrue(first.sent)
        XCTAssertEqual(first.version, version)
        XCTAssertEqual(uploads, 1)

        let second = try installer.install(host: "arch", artifact: artifact)
        XCTAssertFalse(second.sent, "a matching binary is reused, not re-sent")
        XCTAssertEqual(uploads, 1, "no upload on the second connection")
    }

    func testInstallsCanBeRefused() throws {
        let artifact = try fakeArtifact()
        defer { try? FileManager.default.removeItem(at: artifact) }
        let installer = HostInstaller(run: { _, script, _ in
            if script.contains("uname -sm") {
                return HostInstaller.Result(status: 0, output: "Linux x86_64", error: "")
            }
            return HostInstaller.Result(status: 0, output: "", error: "")
        })
        XCTAssertThrowsError(try installer.install(host: "arch", artifact: artifact, allowInstall: false)) {
            XCTAssertTrue("\($0)".contains("refused"), "got \($0)")
        }
    }

    func testAnUnsupportedPlatformIsReportedNotGuessed() throws {
        let artifact = try fakeArtifact()
        defer { try? FileManager.default.removeItem(at: artifact) }
        let installer = HostInstaller(run: { _, _, _ in
            HostInstaller.Result(status: 0, output: "SunOS sparc", error: "")
        })
        XCTAssertThrowsError(try installer.install(host: "ancient", artifact: artifact)) {
            XCTAssertTrue("\($0)".contains("SunOS sparc"), "got \($0)")
        }
    }

    func testARemoteHashMismatchIsAFailureWithTheRemoteReason() throws {
        let artifact = try fakeArtifact()
        defer { try? FileManager.default.removeItem(at: artifact) }
        let installer = HostInstaller(run: { _, script, _ in
            if script.contains("uname -sm") {
                return HostInstaller.Result(status: 0, output: "Linux x86_64", error: "")
            }
            if script.contains("set -e") {
                return HostInstaller.Result(
                    status: 3, output: "", error: "pigeon-host: hash mismatch (got deadbeef)")
            }
            return HostInstaller.Result(status: 0, output: "", error: "")
        })
        XCTAssertThrowsError(try installer.install(host: "arch", artifact: artifact)) {
            XCTAssertTrue("\($0)".contains("hash mismatch"), "got \($0)")
        }
    }

    // MARK: - against a real ssh host (PIGEON_SSH_HOST=arch)

    /// The whole point of the installer is that it works on a machine we do not control, so
    /// the end-to-end check is a real ssh session. Opt-in: `PIGEON_SSH_HOST=arch swift test`.
    func testRealRemoteInstallIsCachedVerifiedAndConfined() throws {
        guard let host = ProcessInfo.processInfo.environment["PIGEON_SSH_HOST"], !host.isEmpty else {
            throw XCTSkip("set PIGEON_SSH_HOST to run the remote install check")
        }
        let installer = HostInstaller()
        let platform = try installer.platform(of: host)
        guard let artifact = HostInstaller.artifact(
            for: platform, in: HostInstaller.searchDirectories())
        else {
            throw XCTSkip("no local binary for \(platform); build one with `cargo build --release --target x86_64-unknown-linux-musl`")
        }

        // A random suffix gives a fresh version hash on every run, so "first connection
        // uploads" keeps holding instead of depending on what the previous run left on the
        // host. Trailing bytes after an ELF are ignored, so the copy still runs.
        let copy = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-host-e2e-\(UUID().uuidString)")
        try (Data(contentsOf: artifact) + Data(UUID().uuidString.utf8)).write(to: copy)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: copy.path)
        defer { try? FileManager.default.removeItem(at: copy) }

        let first = try installer.install(host: host, artifact: copy)
        XCTAssertTrue(first.sent, "first connection uploads")
        XCTAssertEqual(first.platform, platform)
        print("installed at \(first.path) (version \(first.version))")

        let second = try installer.install(host: host, artifact: copy)
        XCTAssertFalse(second.sent, "second connection must not re-upload")
        XCTAssertEqual(second.path, first.path)

        // A transfer that does not match the expected hash must leave nothing behind.
        let bogus = HostInstaller(
            run: { host, script, stdin in
                try HostInstaller.sshRunner(host: host, script: script, stdin: stdin)
            },
            hash: { _ in String(repeating: "0", count: 64) })
        XCTAssertThrowsError(try bogus.install(host: host, artifact: copy)) { error in
            XCTAssertTrue("\(error)".contains("hash mismatch"), "got \(error)")
        }
        let leftovers = try HostInstaller.sshRunner(
            host: host,
            script: "ls -1 ~/.pigeon/host/*/pigeon-host.tmp 2>/dev/null | wc -l",
            stdin: nil)
        XCTAssertEqual(leftovers.output.trimmingCharacters(in: .whitespacesAndNewlines), "0")

        // Everything the install wrote lives under ~/.pigeon. The listing runs from a
        // helper script that itself sits in that directory, so `install.sh` is expected
        // there and must not be mistaken for the install's own debris.
        let stray = try HostInstaller.sshRunner(
            host: host,
            script: "ls -1 ~/.pigeon | grep -v '^install\\.sh$' | tr '\\n' ' '",
            stdin: nil)
        XCTAssertEqual(stray.output.trimmingCharacters(in: .whitespacesAndNewlines), "host")
        let versions = try HostInstaller.sshRunner(
            host: host,
            script: "ls -1 ~/.pigeon/host | wc -l",
            stdin: nil)
        XCTAssertLessThanOrEqual(
            Int(versions.output.trimmingCharacters(in: .whitespacesAndNewlines)) ?? 99, 2,
            "only the newest two versions are kept")
        let runnable = try HostInstaller.sshRunner(host: host, script: "\(first.path) --help", stdin: nil)
        XCTAssertTrue(runnable.ok, "the installed binary runs: \(runnable.error)")
        XCTAssertTrue(runnable.output.contains("--proxy"))
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
        let file = directory.appendingPathComponent("pigeon-host-x86_64-unknown-linux-musl")
        FileManager.default.createFile(atPath: file.path, contents: Data("not really an elf".utf8))
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
        return file
    }
}
