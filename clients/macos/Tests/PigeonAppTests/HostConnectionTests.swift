import XCTest
@testable import PigeonApp

final class HostConnectionTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_000)

    func testAReconnectKeepsScreensAsStaleAndForgetsInFlightAttaches() {
        var connection = HostConnection<Int>()
        connection.seen = [1, 2]
        connection.requested = [3]
        connection.reconnected(now: now)
        XCTAssertEqual(connection.stale, [1, 2])
        XCTAssertTrue(connection.requested.isEmpty)
        XCTAssertEqual(connection.heartbeat, Heartbeat(now: now))
    }

    func testANewHostInstanceDropsEverythingResumable() {
        var connection = HostConnection<Int>()
        connection.instanceChanged(to: "a")
        connection.rows = [1: 42]
        connection.seq = [1: 40]
        connection.stale = [1]
        connection.instanceChanged(to: "a")
        XCTAssertEqual(connection.seq, [1: 40], "the same process: keep resuming")
        XCTAssertEqual(connection.rows, [1: 42])
        connection.instanceChanged(to: "b")
        XCTAssertTrue(connection.seq.isEmpty)
        XCTAssertNil(connection.stale)
        XCTAssertTrue(connection.rows.isEmpty, "pane ids and status seq belong to the old host")
    }

    func testReconnectCannotAcknowledgeAStaleScreenBeforeTheHandshake() {
        var connection = HostConnection<Int>()
        connection.instanceChanged(to: "a")
        let ack = StatusAcknowledgement(host: "devbox", instance: "a", pane: 1,
                                        record: StatusRecord(seq: 1, record: "task"))
        XCTAssertTrue(connection.accepts(ack, host: "devbox"))
        connection.reconnected(now: now)
        XCTAssertFalse(connection.accepts(ack, host: "devbox"))
        connection.instanceChanged(to: "b")
        XCTAssertFalse(connection.accepts(ack, host: "devbox"))
        XCTAssertFalse(connection.accepts(ack, host: "local"))
    }

    func testCallbacksFromReplacedConnectionsAreIgnored() {
        var connection = HostConnection<Int>()
        connection.reconnected(now: now)
        let original = connection.generation
        connection.reconnected(now: now)
        XCTAssertFalse(connection.accepts(generation: original))
        XCTAssertTrue(connection.accepts(generation: connection.generation))
    }

    func testReconnectNeedsANewScreenBeforeItCanConfirmANewReport() {
        var connection = HostConnection<Int>()
        connection.instanceChanged(to: "a")
        connection.seen = [10]
        connection.receivedScreen.insert(10)
        connection.reconnected(now: now)
        connection.instanceChanged(to: "a")
        XCTAssertEqual(connection.seen, [10], "cached screen remains available for resume")
        XCTAssertTrue(connection.receivedScreen.isEmpty, "cached pixels cannot confirm a fresh report")
        connection.receivedScreen.insert(10)
        XCTAssertTrue(connection.receivedScreen.contains(10))
        connection.disconnected()
        XCTAssertTrue(connection.receivedScreen.isEmpty)
    }

    func testReconnectDiscardsSnapshotsWaitingForThePreviousConnectionsLayout() {
        var connection = HostConnection<Int>()
        connection.instanceChanged(to: "a")
        connection.pending = [10: Data([1, 2, 3])]
        connection.reconnected(now: now)
        connection.instanceChanged(to: "a")
        XCTAssertTrue(connection.pending.isEmpty, "a new layout must not qualify an old buffered picture")
    }

    func testHostsWithoutAnInstanceCannotReuseIdentityAcrossConnections() {
        var connection = HostConnection<Int>()
        connection.instanceChanged(to: "")
        let first = connection.instance
        connection.rows = [1: 42]
        connection.reconnected(now: now)
        connection.instanceChanged(to: "")
        XCTAssertNotEqual(connection.instance, first)
        XCTAssertTrue(connection.rows.isEmpty)
    }
}
