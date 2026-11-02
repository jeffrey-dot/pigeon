import XCTest
@testable import PigeonApp

final class HeartbeatTests: XCTestCase {
    private let start = Date(timeIntervalSince1970: 1_000_000)

    func testABusyLinkNeverPings() {
        var beat = Heartbeat(now: start)
        for second in stride(from: 1.0, to: 120, by: 5) {
            let now = start.addingTimeInterval(second)
            beat.heard(at: now)
            XCTAssertEqual(beat.tick(at: now), .none)
        }
    }

    func testSilencePingsOncePerIntervalThenGivesUp() {
        var beat = Heartbeat(now: start)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(10)), .none)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(15)), .ping)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(20)), .none, "one ping per interval")
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(30)), .ping)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(45)), .dead)
    }

    func testAnAnswerResetsTheClock() {
        var beat = Heartbeat(now: start)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(16)), .ping)
        beat.heard(at: start.addingTimeInterval(17))
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(50)), .ping, "45s counts from the pong")
    }

    func testAClockThatJumpsBackIsNotADeadLink() {
        var beat = Heartbeat(now: start)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(-3600)), .none)
        XCTAssertEqual(beat.tick(at: start.addingTimeInterval(-3600 + 10)), .none)
    }
}
