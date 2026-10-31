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
        connection.seq = [1: 40]
        connection.stale = [1]
        connection.instanceChanged(to: "a")
        XCTAssertEqual(connection.seq, [1: 40], "the same process: keep resuming")
        connection.instanceChanged(to: "b")
        XCTAssertTrue(connection.seq.isEmpty)
        XCTAssertNil(connection.stale)
    }
}
