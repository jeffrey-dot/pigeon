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
        XCTAssertNil(encode(KeyStroke(key: .function(21))), "F21+ is not a key we map")
        XCTAssertNil(encode(KeyStroke(key: .function(0))))
    }

    /// xterm-256color's terminfo: kf13–kf20 are Shift+F1–F8.
    func testF13ToF20AreShiftedF1ToF8() {
        XCTAssertEqual(encode(KeyStroke(key: .function(13))), "\u{1b}[1;2P")
        XCTAssertEqual(encode(KeyStroke(key: .function(15))), "\u{1b}[1;2R")
        XCTAssertEqual(encode(KeyStroke(key: .function(17))), "\u{1b}[15;2~")
        XCTAssertEqual(encode(KeyStroke(key: .function(20))), "\u{1b}[19;2~")
        XCTAssertEqual(encode(KeyStroke(key: .function(13), control: true)), "\u{1b}[1;6P")
    }

    func testInsertIsTilde2() {
        XCTAssertEqual(encode(KeyStroke(key: .insert)), "\u{1b}[2~")
        XCTAssertEqual(encode(KeyStroke(key: .insert, control: true)), "\u{1b}[2;5~")
    }

    /// xterm's Ctrl+digit table (VT220 heritage): 2 is NUL, 3–7 are ESC and FS–US, 8 is DEL.
    func testControlDigitsAndSlashFollowXterm() {
        let expected: [Character: UInt8] = [
            "/": 0x1f, "2": 0x00, "3": 0x1b, "4": 0x1c, "5": 0x1d, "6": 0x1e, "7": 0x1f,
            "8": 0x7f, "~": 0x1e,
        ]
        for (character, byte) in expected {
            XCTAssertEqual(
                KeyEncoder.encode(KeyStroke(key: .character(character), control: true)), [byte],
                "Ctrl+\(character)")
        }
        // Edge: digits with no control form type themselves.
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .character("1"), control: true)), [0x31])
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .character("9"), control: true)), [0x39])
    }

    func testOptionBackspaceDeletesAWord() {
        XCTAssertEqual(KeyEncoder.encode(KeyStroke(key: .backspace, option: true)), [0x1b, 0x7f])
    }
}

/// Ctrl chords on layouts whose letters are not ASCII (Russian, Greek, Hebrew…).
final class KeyEncoderLayoutTests: XCTestCase {
    func testControlOnACyrillicLetterUsesTheASCIILayoutKey() {
        let stroke = KeyStroke(key: .character("с"), control: true, base: "c")
        XCTAssertEqual(KeyEncoder.encode(stroke), [0x03])
        let meta = KeyStroke(key: .character("с"), control: true, option: true, base: "c")
        XCTAssertEqual(KeyEncoder.encode(meta), [0x1b, 0x03])
    }

    func testWithoutControlTheLayoutCharacterIsTyped() {
        let stroke = KeyStroke(key: .character("с"), base: "c")
        XCTAssertEqual(KeyEncoder.encode(stroke), Array("с".utf8))
    }

    /// Edge: no ASCII-layout key known (translation failed) → the character itself, never
    /// a made-up control byte.
    func testControlWithoutABaseKeyFallsBackToTheCharacter() {
        let stroke = KeyStroke(key: .character("с"), control: true)
        XCTAssertEqual(KeyEncoder.encode(stroke), Array("с".utf8))
    }

    /// The real layout lookup: whatever ASCII layout the machine has, key code 8 (the
    /// physical C key) types an ASCII letter.
    func testTheASCIILayoutTranslatesAPhysicalLetterKey() throws {
        let character = try XCTUnwrap(KeyStroke.asciiCharacter(keyCode: 8))
        XCTAssertNotNil(character.asciiValue)
        XCTAssertTrue(character.isLetter)
    }

    func testARussianControlEventCarriesTheBaseKey() throws {
        let event = try XCTUnwrap(NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: .control, timestamp: 0,
            windowNumber: 0, context: nil, characters: "с", charactersIgnoringModifiers: "с",
            isARepeat: false, keyCode: 8))
        let stroke = try XCTUnwrap(KeyStroke(event))
        XCTAssertEqual(stroke.key, .character("с"))
        XCTAssertNotNil(stroke.base)
    }
}
