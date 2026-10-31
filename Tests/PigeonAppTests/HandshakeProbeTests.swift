import XCTest
@testable import PigeonApp

final class HandshakeProbeTests: XCTestCase {
    func testAnAckBeforeTheDeadlineCounts() {
        let probe = HandshakeProbe()
        DispatchQueue.global().async { probe.finish(answered: true) }
        XCTAssertTrue(probe.wait(seconds: 2))
    }

    func testAnAckAfterTheTimeoutIsIgnoredAndHarmless() {
        let probe = HandshakeProbe()
        XCTAssertFalse(probe.wait(seconds: 0.05))
        probe.finish(answered: true)
        probe.finish(answered: false)
    }

    func testACloseBeforeAnyAckIsARefusal() {
        let probe = HandshakeProbe()
        probe.finish(answered: false)
        probe.finish(answered: true)
        XCTAssertFalse(probe.wait(seconds: 1))
    }
}
