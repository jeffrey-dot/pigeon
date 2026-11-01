import XCTest
@testable import PigeonApp

/// The host list behind the Hosts menu: what it remembers, and that a corrupt file never
/// takes the window down with it.
final class HostsStoreTests: XCTestCase {
    func testHostsAndTheCurrentOneSurviveARestart() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-servers-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("hosts.json")

        let store = HostsStore(url: url)
        XCTAssertEqual(store.load(), HostsState(), "nothing saved yet is an empty list")

        store.save(HostsState(hosts: ["arch", "build-box"], current: "arch"))
        let reloaded = HostsStore(url: url).load()
        XCTAssertEqual(reloaded.hosts, ["arch", "build-box"])
        XCTAssertEqual(reloaded.current, "arch")

        // Switching back to this Mac clears the current host but keeps the list.
        store.save(HostsState(hosts: reloaded.hosts, current: nil))
        XCTAssertNil(HostsStore(url: url).load().current)
        XCTAssertEqual(HostsStore(url: url).load().hosts, ["arch", "build-box"])
    }

    func testACorruptFileFallsBackToAnEmptyList() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-servers-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("hosts.json")
        try Data("not json at all".utf8).write(to: url)

        XCTAssertEqual(HostsStore(url: url).load(), HostsState())
    }

    func testTheDefaultFileLivesUnderTheConfigDirectory() {
        let override = ProcessInfo.processInfo.environment["PIGEON_CONFIG_DIR"]
        guard let override, !override.isEmpty else {
            XCTAssertTrue(
                HostsStore.defaultURL.path.hasSuffix(".config/pigeon/hosts.json"),
                HostsStore.defaultURL.path)
            return
        }
        XCTAssertEqual(HostsStore.defaultURL.path, override + "/hosts.json")
    }

    func testWhatTheUserTypesBecomesOneSshDestination() {
        XCTAssertEqual(try HostName.parse("  jeffrey@asia \n").get(), "jeffrey@asia")
        XCTAssertEqual(try HostName.parse("ssh jeffrey@asia").get(), "jeffrey@asia", "a pasted command")
        XCTAssertEqual(try HostName.parse("ssh asia").get(), "asia")
        XCTAssertEqual(try HostName.parse("sshbox").get(), "sshbox", "only the word ssh is dropped")
        XCTAssertEqual(HostName.parse(""), .failure(.empty))
        XCTAssertEqual(HostName.parse("ssh "), .failure(.empty))
        for bad in ["asia -p 2222", "ssh -p 2222 asia", "-oProxyCommand=x", "a\tb", "a\u{7}b"] {
            guard case .failure(.notOneName) = HostName.parse(bad) else {
                return XCTFail("accepted \(bad)")
            }
        }
        XCTAssertFalse(HostName.Problem.notOneName("asia -p 1").description.contains("ssh -"), "no jargon")
    }

    /// A list saved before names were checked (`ssh asia`) is repaired on load, not retried.
    func testABadSavedHostIsRepairedOrDropped() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-servers-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("hosts.json")
        let saved = #"{"hosts":["arch","ssh asia","jeffrey@asia","asia","x -p 2"],"current":"ssh asia"}"#
        try Data(saved.utf8).write(to: url)
        let loaded = HostsStore(url: url).load()
        XCTAssertEqual(loaded.hosts, ["arch", "asia", "jeffrey@asia"])
        XCTAssertEqual(loaded.current, "asia")
    }
}
