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

/// Modes the program turned on, and xterm's modifier parameter on cursor and function keys.
final class KeyEncoderModeTests: XCTestCase {
    private func encode(_ stroke: KeyStroke, appCursor: Bool = false) -> String? {
        KeyEncoder.encode(stroke, appCursor: appCursor).map { String(decoding: $0, as: UTF8.self) }
    }

    func testApplicationCursorModeSwitchesArrowsToSS3() {
        XCTAssertEqual(encode(KeyStroke(key: .up), appCursor: true), "\u{1b}OA")
        XCTAssertEqual(encode(KeyStroke(key: .left), appCursor: true), "\u{1b}OD")
        XCTAssertEqual(encode(KeyStroke(key: .home), appCursor: true), "\u{1b}OH")
        XCTAssertEqual(encode(KeyStroke(key: .up)), "\u{1b}[A", "normal mode stays CSI")
    }

    func testModifiedArrowsCarryXtermsParameter() {
        // 1 + shift(1) + alt(2) + ctrl(4)
        XCTAssertEqual(encode(KeyStroke(key: .right, control: true)), "\u{1b}[1;5C")
        XCTAssertEqual(encode(KeyStroke(key: .left, option: true)), "\u{1b}[1;3D")
        XCTAssertEqual(encode(KeyStroke(key: .up, shift: true)), "\u{1b}[1;2A")
        XCTAssertEqual(encode(KeyStroke(key: .end, control: true, shift: true)), "\u{1b}[1;6F")
        // A modifier wins over application mode, as in xterm.
        XCTAssertEqual(encode(KeyStroke(key: .up, control: true), appCursor: true), "\u{1b}[1;5A")
    }

    func testModifiedTildeKeysPutTheParameterBeforeTheTilde() {
        XCTAssertEqual(encode(KeyStroke(key: .delete, control: true)), "\u{1b}[3;5~")
        XCTAssertEqual(encode(KeyStroke(key: .function(5), shift: true)), "\u{1b}[15;2~")
        XCTAssertEqual(encode(KeyStroke(key: .pageUp)), "\u{1b}[5~")
    }

    func testTheFirstFourFunctionKeysUseSS3() {
        XCTAssertEqual(encode(KeyStroke(key: .function(1))), "\u{1b}OP")
        XCTAssertEqual(encode(KeyStroke(key: .function(4))), "\u{1b}OS")
        XCTAssertEqual(encode(KeyStroke(key: .function(2), control: true)), "\u{1b}[1;5Q")
        XCTAssertNil(encode(KeyStroke(key: .function(13))), "F13+ is not a key we map")
    }

    func testOptionBackspaceDeletesAWord() {
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .backspace, option: true)), [0x1b, 0x7f])
    }
}
