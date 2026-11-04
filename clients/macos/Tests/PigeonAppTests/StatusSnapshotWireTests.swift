import XCTest
import PigeonCore
@testable import PigeonApp

final class StatusSnapshotWireTests: XCTestCase {
    // Literal protocol fixture, decoded exclusively by core/proto. Includes u64 fields
    // above UInt32.max and UTF-8 to detect truncation and borrowed-string lifetimes.
    private let frame = "7d000000a461746f7374617475735f736e617073686f746770616e655f69640a687265766973696f6e1b0000000100000001677265636f72647381a7626964647461736b637365710265737461746564646f6e65646b696e6460636d73676be4bda0e5a5bd20646f6e6563617070627069666167655f6d731b000000012a05f200"

    private func decode() throws -> OpaquePointer {
        let chars = Array(frame)
        let bytes = stride(from: 0, to: chars.count, by: 2).map {
            UInt8(String(chars[$0...($0 + 1)]), radix: 16)!
        }
        var consumed = 0
        let message = bytes.withUnsafeBufferPointer {
            pg_msg_decode_frame($0.baseAddress, $0.count, &consumed)
        }
        XCTAssertEqual(consumed, bytes.count)
        return try XCTUnwrap(message)
    }

    func testHandleCanBeCopiedAfterSourceMessageIsFreedAndValuesSurviveHandleFree() throws {
        let message = try decode()
        let handle = pg_msg_status_snapshot(message)
        Wire.free(message)
        let owned = try XCTUnwrap(handle)
        let now = Date(timeIntervalSince1970: 10_000_000)
        let copied = StatusSnapshot.copy(handle: owned, now: now)
        pg_status_snapshot_free(owned)
        let snapshot = try XCTUnwrap(copied)
        XCTAssertEqual(snapshot.pane, 10)
        XCTAssertEqual(snapshot.revision, 4_294_967_297)
        XCTAssertEqual(snapshot.records.count, 1)
        XCTAssertEqual(snapshot.records[0].identity, .init(seq: 2, record: "task"))
        XCTAssertEqual(snapshot.records[0].status, .done)
        XCTAssertEqual(snapshot.records[0].agent.message, "你好 done")
        XCTAssertEqual(snapshot.records[0].agent.app, "pi")
        XCTAssertEqual(snapshot.records[0].agent.since, now.addingTimeInterval(-5_000_000))
    }

    func testMessageAdapterCopiesValidSnapshotAndRejectsIncompleteOrWrongType() throws {
        let message = try decode()
        let copied = StatusSnapshot(message: message)
        Wire.free(message)
        XCTAssertEqual(copied?.records.first?.agent.message, "你好 done")
        for type in [PG_MSG_STATUS_SNAPSHOT, PG_MSG_STATUS] {
            let invalid = try XCTUnwrap(Wire.make(type))
            XCTAssertNil(StatusSnapshot(message: invalid))
            Wire.free(invalid)
        }
    }
}
