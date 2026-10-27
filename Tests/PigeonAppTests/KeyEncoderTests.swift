import XCTest
@testable import PigeonApp

final class KeyEncoderTests: XCTestCase {
    func testControlLettersBecomeControlBytes() {
        let stroke = KeyStroke(key: .character("c"), control: true)
        XCTAssertEqual(KeyEncoder.encode(stroke), [0x03])
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .character("["), control: true)), [0x1b])
    }

    func testOptionPrefixesEscape() {
        let stroke = KeyStroke(key: .character("b"), option: true)
        XCTAssertEqual(KeyEncoder.encode(stroke), [0x1b, UInt8(ascii: "b")])
    }

    func testSpecialKeysUseLegacySequences() {
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .enter)), [0x0d])
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .up)), Array("\u{1b}[A".utf8))
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .pageDown)), Array("\u{1b}[6~".utf8))
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .backspace)), [0x7f])
    }

    func testCommandChordsBelongToTheApp() {
        XCTAssertNil(KeyEncoder.encode(KeyStroke(key: .character("q"), command: true)))
    }

    func testUnicodePassesThrough() {
        let stroke = KeyStroke(key: .character("你"))
        XCTAssertEqual(KeyEncoder.encode(stroke), Array("你".utf8))
    }
}
