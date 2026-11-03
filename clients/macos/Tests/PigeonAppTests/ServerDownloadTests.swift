import CryptoKit
import XCTest
@testable import PigeonApp

/// The on-demand server download: right asset per platform, verified before it is used,
/// fetched once per version.
final class ServerDownloadTests: XCTestCase {
    private let base = URL(string: "https://example.invalid/v9.9.9")!

    func testItDownloadsVerifiesAndCachesTheBuildForThePlatform() throws {
        let cache = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: cache) }
        let binary = Data("elf-binary".utf8)
        let gz = try gzip(binary)
        let name = "pigeon-server-aarch64-unknown-linux-musl.gz"
        var fetched: [String] = []
        let download = ServerDownload(
            version: "9.9.9", base: base, cache: cache,
            fetch: { url in
                fetched.append(url.lastPathComponent)
                if url.lastPathComponent == "SHA256SUMS" {
                    return Data("\(Self.hex(gz))  \(name)\n".utf8)
                }
                XCTAssertEqual(url.lastPathComponent, name, "only the matching platform is fetched")
                return gz
            })

        let first = try download.binary(for: "Linux aarch64")
        XCTAssertEqual(try Data(contentsOf: first), binary)
        XCTAssertTrue(FileManager.default.isExecutableFile(atPath: first.path))
        XCTAssertTrue(first.path.contains("/9.9.9/"), "cached per version")

        let again = try download.binary(for: "Linux aarch64")
        XCTAssertEqual(again, first)
        XCTAssertEqual(fetched, ["SHA256SUMS", name], "the second connect downloads nothing")
    }

    func testATamperedDownloadIsRejectedAndLeavesNothing() throws {
        let cache = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: cache) }
        let name = "pigeon-server-x86_64-unknown-linux-musl.gz"
        let download = ServerDownload(
            version: "9.9.9", base: base, cache: cache,
            fetch: { url in
                if url.lastPathComponent == "SHA256SUMS" {
                    return Data("\(String(repeating: "0", count: 64))  \(name)\n".utf8)
                }
                return try self.gzip(Data("evil".utf8))
            })
        XCTAssertThrowsError(try download.binary(for: "Linux x86_64")) { error in
            XCTAssertTrue("\(error)".contains("does not match"), "\(error)")
        }
        let left = try FileManager.default.contentsOfDirectory(atPath: cache.path)
        XCTAssertTrue(left.isEmpty, "nothing unverified is written: \(left)")
    }

    func testAnUnknownPlatformOrAMissingEntryIsAnError() throws {
        let download = ServerDownload(
            version: "9.9.9", base: base, cache: try temporaryDirectory(),
            fetch: { _ in Data("garbage line\n".utf8) })
        XCTAssertThrowsError(try download.binary(for: "Linux riscv64"))
        XCTAssertThrowsError(try download.binary(for: "Linux x86_64")) { error in
            XCTAssertTrue("\(error)".contains("not in the release"), "\(error)")
        }
    }

    /// The checksums come from the same place as the binary, so the transport is the only
    /// thing vouching for them: plain http (anywhere but this machine) is refused before
    /// anything is fetched.
    func testAMirrorMustUseHttps() throws {
        for mirror in ["http://mirror.example/pigeon", "ftp://mirror.example/pigeon", "file:///tmp/pigeon"] {
            var fetched = false
            let download = ServerDownload(
                version: "9.9.9", base: URL(string: mirror), cache: try temporaryDirectory(),
                fetch: { _ in
                    fetched = true
                    return Data()
                })
            XCTAssertThrowsError(try download.binary(for: "Linux x86_64"), mirror) { error in
                XCTAssertTrue("\(error)".contains("https"), "\(error)")
            }
            XCTAssertFalse(fetched, mirror)
        }
        for allowed in ["https://mirror.example/p", "http://localhost:8000/p", "http://127.0.0.1/p", "http://[::1]/p"] {
            XCTAssertTrue(ServerDownload.isTrusted(try XCTUnwrap(URL(string: allowed))), allowed)
        }
        XCTAssertFalse(ServerDownload.isTrusted(try XCTUnwrap(URL(string: "http://localhost.evil.example/p"))))
    }

    func testChecksumLinesAreReadWholeOrNotAtAll() {
        let good = String(repeating: "a", count: 64)
        let sums = ServerDownload.parseSums("""
            \(good)  pigeon-server-x86_64-unknown-linux-musl.gz
            \(good.uppercased()) *pigeon-server-aarch64-apple-darwin.gz
            abc  short-hash.gz
            \(String(repeating: "z", count: 64))  not-hex.gz
            \(good)  two names.gz

            """)
        XCTAssertEqual(sums["pigeon-server-x86_64-unknown-linux-musl.gz"], good)
        XCTAssertEqual(sums["pigeon-server-aarch64-apple-darwin.gz"], good, "binary-mode marker")
        XCTAssertEqual(sums.count, 2, "malformed lines are dropped: \(sums)")
    }

    func testOnlyAVersionedAppDownloads() {
        XCTAssertNil(ServerDownload.standard(environment: [:], bundleVersion: nil), "dev build")
        XCTAssertEqual(ServerDownload.standard(environment: [:], bundleVersion: "0.2.0")?.version, "0.2.0")
        let mirrored = ServerDownload.standard(
            environment: ["PIGEON_SERVER_URL": "https://mirror.example/pigeon"], bundleVersion: "0.2.0")
        XCTAssertEqual(mirrored?.base.absoluteString, "https://mirror.example/pigeon")
        XCTAssertEqual(
            ServerDownload(version: "0.2.0").base.absoluteString,
            "https://github.com/jeffrey-dot/pigeon/releases/download/v0.2.0")
    }

    /// Every platform the app can serve has a release build, and the release builds nothing
    /// the app would never ask for: the workflow matrix and `HostInstaller.platforms` agree.
    func testTheReleaseBuildsExactlyThePlatformsTheAppServes() throws {
        let workflow = RepoRoot.file(".github/workflows/release.yml")
        let text = try String(contentsOf: workflow, encoding: .utf8)
        let built = Set(text.split(whereSeparator: \.isNewline).compactMap { line -> String? in
            guard let range = line.range(of: "triple: ") else { return nil }
            let rest = line[range.upperBound...]
            return rest.split(whereSeparator: { $0 == "," || $0 == "}" }).first.map(String.init)
        })
        XCTAssertEqual(built, Set(HostInstaller.triples))
    }

    func testCommonServerCpusAreAllCovered() {
        for platform in ["Linux x86_64", "Linux aarch64", "Linux armv7l", "Linux armv6l", "Linux i686"] {
            XCTAssertNotNil(HostInstaller.triple(for: platform), platform)
        }
        XCTAssertEqual(HostInstaller.triple(for: "Linux arm64"), HostInstaller.triple(for: "Linux aarch64"))
        XCTAssertNil(HostInstaller.triple(for: "Linux s390x"), "unbuilt CPUs fall back to plain ssh")
    }

    private static func hex(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    private func gzip(_ data: Data) throws -> Data {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try data.write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/gzip")
        process.arguments = ["-c", file.path]
        let out = Pipe()
        process.standardOutput = out
        try process.run()
        let result = out.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return result
    }

    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-download-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}
