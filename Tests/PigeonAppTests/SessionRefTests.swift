import XCTest
@testable import PigeonApp

final class SessionRefTests: XCTestCase {
    func testKeysRoundTripIncludingHostsWithColons() {
        for ref in [
            SessionRef(host: "local", session: 3),
            SessionRef(host: "devbox", session: 12),
            SessionRef(host: "ada@[::1]:2222", session: 4),
        ] {
            XCTAssertEqual(SessionRef(key: ref.key), ref, ref.key)
        }
        XCTAssertEqual(SessionRef(host: "local", session: 3).key, "3", "the key older builds wrote")
    }

    func testMalformedKeysAreRefused() {
        for key in ["", "x", "0", "-2", "devbox:", ":4", "devbox:0", "devbox:two"] {
            XCTAssertNil(SessionRef(key: key), key)
        }
    }

    func testIdsAreStableAndNeverReused() {
        var refs = SessionRefs()
        let a = refs.id(for: SessionRef(host: "local", session: 1))
        let b = refs.id(for: SessionRef(host: "devbox", session: 1))
        XCTAssertNotEqual(a, b, "the same session number on two hosts")
        XCTAssertEqual(refs.id(for: SessionRef(host: "local", session: 1)), a)
        XCTAssertEqual(refs.ref(for: b), SessionRef(host: "devbox", session: 1))
        XCTAssertNil(refs.ref(for: 999))
        XCTAssertNil(refs.existingID(for: SessionRef(host: "other", session: 1)))
    }
}
