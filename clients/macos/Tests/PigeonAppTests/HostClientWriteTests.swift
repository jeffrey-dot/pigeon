import Darwin
import XCTest
import PigeonCore
@testable import PigeonApp

/// Writes leave the caller's thread: frames arrive whole and in order, the caller never
/// blocks on a full socket, and a peer that stops reading is dropped instead of queueing
/// without bound.
final class HostClientWriteTests: XCTestCase {
    private func pair() -> (Int32, Int32) {
        var fds: [Int32] = [0, 0]
        XCTAssertEqual(socketpair(AF_UNIX, SOCK_STREAM, 0, &fds), 0)
        return (fds[0], fds[1])
    }

    private func keys(_ bytes: [UInt8]) -> OpaquePointer {
        let message = Wire.make(PG_MSG_KEYS)!
        Wire.set(message, u32: "pane_id", 1)
        Wire.set(message, bytes: "bytes", bytes)
        return message
    }

    func testFramesArriveWholeAndInOrder() throws {
        let (mine, theirs) = pair()
        let client = HostClient(read: mine, write: mine)
        defer { client.close(); Darwin.close(theirs) }
        var expected = Data()
        for index in 0..<50 {
            let message = keys(Array(repeating: UInt8(index), count: 100))
            expected.append(try XCTUnwrap(Wire.frame(message)))
            client.send(message)
            Wire.free(message)
        }
        client.flushWrites()
        var received = Data()
        var buffer = [UInt8](repeating: 0, count: 64 * 1024)
        while received.count < expected.count {
            let count = Darwin.read(theirs, &buffer, buffer.count)
            guard count > 0 else { break }
            received.append(contentsOf: buffer[0..<count])
        }
        XCTAssertEqual(received, expected)
    }

    func testAPeerThatNeverReadsIsDroppedWithoutBlockingTheCaller() {
        let (mine, theirs) = pair()
        let client = HostClient(read: mine, write: mine)
        defer { Darwin.close(theirs) }
        let closed = expectation(description: "closed")
        closed.assertForOverFulfill = false
        client.onClose = { closed.fulfill() }
        let message = keys(Array(repeating: 0x61, count: 1 << 20))
        defer { Wire.free(message) }
        let start = Date()
        // 1 MiB frames into a socket nobody reads: past the outbox limit the client gives up.
        for _ in 0..<(HostClient.maxQueued >> 20) + 4 {
            client.send(message)
        }
        XCTAssertLessThan(Date().timeIntervalSince(start), 1, "send must not wait for the peer")
        wait(for: [closed], timeout: 5)
    }
}
