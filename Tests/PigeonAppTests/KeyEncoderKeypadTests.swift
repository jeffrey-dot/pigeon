import AppKit
import XCTest
@testable import PigeonApp

/// DECKPAM (`ESC =`): the numeric keypad sends `SS3` sequences that vim, emacs and mc bind.
final class KeyEncoderKeypadTests: XCTestCase {
    private let keypadMode = TerminalModes.appKeypad

    private func encode(_ stroke: KeyStroke, modes: UInt32) -> String? {
        KeyEncoder.encode(stroke, modes: modes).map { String(decoding: $0, as: UTF8.self) }
    }

    func testApplicationKeypadFollowsXterm() {
        let expected: [Character: String] = [
            "0": "p", "1": "q", "5": "u", "9": "y", ".": "n", "*": "j", "+": "k", "-": "m",
            "/": "o", "=": "X",
        ]
        for (character, final) in expected {
            XCTAssertEqual(
                encode(KeyStroke(key: .character(character), keypad: true), modes: keypadMode),
                "\u{1b}O\(final)", "keypad \(character)")
        }
        XCTAssertEqual(encode(KeyStroke(key: .enter, keypad: true), modes: keypadMode), "\u{1b}OM")
    }

    func testNumericKeypadModeTypesTheCharacter() {
        XCTAssertEqual(encode(KeyStroke(key: .character("5"), keypad: true), modes: 0), "5")
        XCTAssertEqual(encode(KeyStroke(key: .enter, keypad: true), modes: 0), "\r")
    }

    /// Edge: only keypad keys change; the main row's digits and Return stay text, and a
    /// modified keypad key keeps its normal encoding.
    func testMainKeyboardKeysIgnoreKeypadMode() {
        XCTAssertEqual(encode(KeyStroke(key: .character("5")), modes: keypadMode), "5")
        XCTAssertEqual(encode(KeyStroke(key: .enter), modes: keypadMode), "\r")
        XCTAssertEqual(
            encode(KeyStroke(key: .character("5"), shift: true, keypad: true), modes: keypadMode),
            "5")
    }
}

/// The AppKit half: key codes 65–92 are the keypad; arrows are not, despite `.numericPad`.
final class PaneKeypadTests: XCTestCase {
    private func event(_ characters: String, keyCode: UInt16) -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: .numericPad, timestamp: 0,
            windowNumber: 0, context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: false, keyCode: keyCode)!
    }

    func testKeypadEnterAndDigitsUseApplicationKeypadWhenAsked() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }
        pane.modes = TerminalModes.appKeypad

        pane.keyDown(with: event("\r", keyCode: 76))
        XCTAssertEqual(received, Array("\u{1b}OM".utf8))
        pane.keyDown(with: event("7", keyCode: 89))
        XCTAssertEqual(received, Array("\u{1b}Ow".utf8))
    }

    func testArrowsAreNotKeypadKeys() {
        XCTAssertEqual(KeyStroke(event("\u{F700}", keyCode: 126))?.keypad, false)
        XCTAssertEqual(KeyStroke(event("\r", keyCode: 36))?.keypad, false)
    }
}
