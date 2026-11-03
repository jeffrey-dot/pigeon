import CryptoKit
import XCTest
@testable import PigeonApp

/// Software update: only a signed, newer release is offered, and only the archive that
/// signature vouches for is installed.
final class UpdaterTests: XCTestCase {
    private let key = Curve25519.Signing.PrivateKey()

    func testVersionsCompareNumerically() {
        XCTAssertLessThan(AppVersion("0.9.0")!, AppVersion("0.10.0")!)
        XCTAssertLessThan(AppVersion("0.1.9")!, AppVersion("0.2")!)
        XCTAssertEqual(AppVersion("1.2"), AppVersion("1.2.0"))
        XCTAssertFalse(AppVersion("1.0.0")! < AppVersion("1.0")!)
        for bad in ["", "v1.0", "1..0", "1.0.0-beta", "1.0.", " 1.0", "1.0.0.0.0", "１.0"] {
            XCTAssertNil(AppVersion(bad), bad)
        }
    }

    func testOnlyANewerSignedReleaseIsOffered() throws {
        let newer = signed(version: "0.3.0")
        XCTAssertEqual(try updater(current: "0.2.0", serving: newer).check()?.version, "0.3.0")
        XCTAssertNil(try updater(current: "0.3.0", serving: newer).check(), "same version")
        XCTAssertNil(try updater(current: "0.4.0", serving: newer).check(), "never a downgrade")
    }

    func testAManifestSignedByAnotherKeyIsRejected() {
        let forged = signed(version: "9.0.0", by: Curve25519.Signing.PrivateKey())
        XCTAssertThrowsError(try updater(current: "0.1.0", serving: forged).check()) {
            XCTAssertTrue("\($0)".contains("safety check"), "\($0)")
        }
    }

    /// The signature binds version and hash together: changing either breaks it.
    func testEditingASignedManifestBreaksTheSignature() throws {
        var manifest = try XCTUnwrap(UpdateManifest.parse(signed(version: "0.3.0")))
        XCTAssertTrue(manifest.isSigned(by: key.publicKey))
        manifest.version = "0.4.0"
        XCTAssertFalse(manifest.isSigned(by: key.publicKey))
        manifest.version = "0.3.0"
        manifest.sha256 = String(repeating: "b", count: 64)
        XCTAssertFalse(manifest.isSigned(by: key.publicKey))
        manifest.signature = "not base64!"
        XCTAssertFalse(manifest.isSigned(by: key.publicKey))
    }

    func testMalformedManifestsAreRejectedWhole() {
        let hash = String(repeating: "a", count: 64)
        let good = #"{"version":"0.2.0","url":"https://x.invalid/a.zip","sha256":"\#(hash)","signature":"c2ln"}"#
        XCTAssertNotNil(UpdateManifest.parse(Data(good.utf8)))
        for bad in [
            "", "{}", "[]", "not json",
            good.replacingOccurrences(of: "https://", with: "http://"),
            good.replacingOccurrences(of: "0.2.0", with: "latest"),
            good.replacingOccurrences(of: hash, with: hash.uppercased()),
            good.replacingOccurrences(of: hash, with: "abc"),
            good.replacingOccurrences(of: #","signature":"c2ln""#, with: ""),
        ] {
            XCTAssertNil(UpdateManifest.parse(Data(bad.utf8)), bad)
        }
    }

    func testCheckingWhileOfflineIsANetworkFailure() {
        let offline = Updater(
            current: "0.1.0", bundleIdentifier: "dev.pigeon.app", publicKey: key.publicKey,
            fetch: { _ in throw URLError(.notConnectedToInternet) })
        XCTAssertThrowsError(try offline.check()) {
            XCTAssertTrue("\($0)".contains("online"), "\($0)")
        }
    }

    /// End to end on disk: a signed archive is downloaded, unpacked next to the app it
    /// replaces, checked, and swapped in; a tampered archive never gets that far.
    func testASignedArchiveReplacesTheAppAndATamperedOneDoesNot() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let installed = try makeApp(in: root.appendingPathComponent("Applications"), version: "0.1.0")
        let archive = try zip(makeApp(in: root.appendingPathComponent("build"), version: "0.2.0"))
        let manifestData = signed(version: "0.2.0", sha256: ServerDownload.sha256(archive))

        let tampered = updater(current: "0.1.0", serving: manifestData, archive: archive + Data([0]))
        let offered = try XCTUnwrap(try tampered.check())
        XCTAssertThrowsError(try tampered.download(offered, near: installed)) {
            XCTAssertTrue("\($0)".contains("safety check"), "\($0)")
        }

        let honest = updater(current: "0.1.0", serving: manifestData, archive: archive)
        let app = try honest.download(try XCTUnwrap(try honest.check()), near: installed)
        try Updater.install(app, replacing: installed)
        let info = try XCTUnwrap(NSDictionary(contentsOf: installed.appendingPathComponent("Contents/Info.plist")))
        XCTAssertEqual(info["CFBundleShortVersionString"] as? String, "0.2.0")
        XCTAssertFalse(FileManager.default.fileExists(atPath: app.path), "the scratch copy is gone")
    }

    /// A correctly signed archive holding some other app (or another version) is refused.
    func testTheArchiveMustHoldThisAppAtTheSignedVersion() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let installed = try makeApp(in: root.appendingPathComponent("Applications"), version: "0.1.0")
        for (identifier, version) in [("com.example.other", "0.2.0"), ("dev.pigeon.app", "0.1.5")] {
            let archive = try zip(makeApp(
                in: root.appendingPathComponent("build-\(version)"), version: version, identifier: identifier))
            let manifest = signed(version: "0.2.0", sha256: ServerDownload.sha256(archive))
            let updater = updater(current: "0.1.0", serving: manifest, archive: archive)
            XCTAssertThrowsError(try updater.download(try XCTUnwrap(try updater.check()), near: installed))
        }
    }

    func testATranslocatedCopyIsNotReplaced() {
        let translocated = URL(fileURLWithPath: "/private/var/folders/x/AppTranslocation/ABC/d/Pigeon.app")
        XCTAssertThrowsError(try Updater.install(URL(fileURLWithPath: "/tmp/x"), replacing: translocated)) {
            XCTAssertTrue("\($0)".contains("Applications"), "\($0)")
        }
    }

    func testOnlyAReleaseBuildUpdates() {
        XCTAssertNil(Updater.standard(info: nil), "swift run")
        XCTAssertNil(Updater.standard(info: ["CFBundleShortVersionString": "dev", "CFBundleIdentifier": "x"]))
        let release = Updater.standard(
            environment: ["PIGEON_UPDATE_URL": "https://mirror.example/latest.json"],
            info: ["CFBundleShortVersionString": "0.2.0", "CFBundleIdentifier": "dev.pigeon.app"])
        XCTAssertEqual(release?.current, "0.2.0")
        XCTAssertEqual(release?.manifestURL.absoluteString, "https://mirror.example/latest.json")
        XCTAssertEqual(
            Updater.standard(info: ["CFBundleShortVersionString": "0.2.0", "CFBundleIdentifier": "x"])?
                .manifestURL, Updater.manifestURL)
    }

    /// The release workflow signs exactly the message the app verifies.
    func testTheReleaseWorkflowSignsWhatTheAppChecks() throws {
        let workflow = RepoRoot.file(".github/workflows/release.yml")
        let text = try String(contentsOf: workflow, encoding: .utf8)
        XCTAssertTrue(text.contains(#"printf 'pigeon-update %s %s' "$version" "$sha""#), "signed message")
        XCTAssertTrue(text.contains("latest.json"))
        XCTAssertTrue(text.contains("Pigeon-$version.zip"))
    }

    // MARK: - helpers

    private func signed(
        version: String, sha256: String = String(repeating: "a", count: 64),
        by signer: Curve25519.Signing.PrivateKey? = nil
    ) -> Data {
        let message = Data("pigeon-update \(version) \(sha256)".utf8)
        let signature = try! (signer ?? key).signature(for: message).base64EncodedString()
        return Data(#"""
            {"version":"\#(version)","url":"https://example.invalid/Pigeon-\#(version).zip",
             "sha256":"\#(sha256)","signature":"\#(signature)"}
            """#.utf8)
    }

    private func updater(current: String, serving manifest: Data, archive: Data = Data()) -> Updater {
        Updater(
            current: current, bundleIdentifier: "dev.pigeon.app", publicKey: key.publicKey,
            manifestURL: URL(string: "https://example.invalid/latest.json")!,
            fetch: { url in url.lastPathComponent == "latest.json" ? manifest : archive })
    }

    private func makeApp(in folder: URL, version: String, identifier: String = "dev.pigeon.app") throws -> URL {
        let app = folder.appendingPathComponent("Pigeon.app")
        let macOS = app.appendingPathComponent("Contents/MacOS")
        try FileManager.default.createDirectory(at: macOS, withIntermediateDirectories: true)
        let binary = macOS.appendingPathComponent("pigeon")
        try Data("#!/bin/sh\n".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
        let info: NSDictionary = [
            "CFBundleIdentifier": identifier, "CFBundleShortVersionString": version,
            "CFBundleExecutable": "pigeon",
        ]
        try info.write(to: app.appendingPathComponent("Contents/Info.plist"))
        return app
    }

    /// The same command the release workflow runs.
    private func zip(_ app: URL) throws -> Data {
        let out = app.deletingLastPathComponent().appendingPathComponent("app.zip")
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        process.arguments = ["-c", "-k", "--keepParent", app.path, out.path]
        try process.run()
        process.waitUntilExit()
        return try Data(contentsOf: out)
    }

    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-update-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}
