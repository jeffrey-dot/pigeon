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

    /// Malformed input: a key event with no characters (dead key / unknown
    /// keyCode) must be ignored rather than produce garbage bytes.
    func testKeyWithoutCharactersIsIgnored() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }

        let event = NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: [],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: "",
            charactersIgnoringModifiers: "",
            isARepeat: false,
            keyCode: 10
        )!
        pane.keyDown(with: event)
        XCTAssertTrue(received.isEmpty, "no bytes for a key we cannot map")
    }

    func testUnicodePassesThrough() {
        let stroke = KeyStroke(key: .character("你"))
        XCTAssertEqual(KeyEncoder.encode(stroke), Array("你".utf8))
    }
}
