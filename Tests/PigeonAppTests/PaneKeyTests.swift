import AppKit
import XCTest
@testable import PigeonApp

/// The AppKit half of the input path: NSEvent → KeyStroke → bytes → PaneView.
/// (The real keyboard cannot be driven in CI, so the event mapping is tested here
/// and the wire half is covered by `core/tests/host.rs`.)
final class PaneKeyTests: XCTestCase {
    private func keyDown(
        _ characters: String,
        keyCode: UInt16 = 0,
        flags: NSEvent.ModifierFlags = []
    ) -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: flags,
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: false,
            keyCode: keyCode
        )!
    }

    func testPrintableKeyIsForwarded() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }

        pane.keyDown(with: keyDown("a", keyCode: 0))
        XCTAssertEqual(received, Array("a".utf8))
    }

    func testControlCBecomesControlByte() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }

        pane.keyDown(with: keyDown("c", keyCode: 8, flags: .control))
        XCTAssertEqual(received, [0x03])
    }

    func testArrowKeyUsesItsLegacySequence() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }

        // 126 is the AppKit key code for the up arrow.
        pane.keyDown(with: keyDown("\u{F700}", keyCode: 126))
        XCTAssertEqual(received, Array("\u{1b}[A".utf8))
    }

    /// Kitty flag 2: a released key reaches the program; without the flag keyUp sends nothing.
    func testKeyUpReportsAReleaseOnlyWhenAsked() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }
        let up = NSEvent.keyEvent(
            with: .keyUp, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0,
            context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}",
            isARepeat: false, keyCode: 53)!

        pane.keyUp(with: up)
        XCTAssertTrue(received.isEmpty)
        pane.modes = 3 << TerminalModes.kittyShift
        pane.keyUp(with: up)
        XCTAssertEqual(received, Array("\u{1b}[27;1:3u".utf8))
    }

    /// Kitty flag 8: Shift alone is reported, press then release.
    func testModifierKeysReportUnderAllKeys() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var received: [UInt8] = []
        pane.onInput = { received = $0 }
        pane.modes = 10 << TerminalModes.kittyShift
        func shift(_ flags: NSEvent.ModifierFlags) -> NSEvent {
            NSEvent.keyEvent(
                with: .flagsChanged, location: .zero, modifierFlags: flags, timestamp: 0,
                windowNumber: 0, context: nil, characters: "", charactersIgnoringModifiers: "",
                isARepeat: false, keyCode: 56)!
        }
        pane.flagsChanged(with: shift(NSEvent.ModifierFlags(rawValue: NSEvent.ModifierFlags.shift.rawValue | 0x02)))
        XCTAssertEqual(received, Array("\u{1b}[57441;2u".utf8))
        pane.flagsChanged(with: shift([]))
        XCTAssertEqual(received, Array("\u{1b}[57441;1:3u".utf8))
    }

    /// Mac key codes for Insert (the Help key position) and F13–F20.
    func testInsertAndHighFunctionKeysMapFromTheirKeyCodes() {
        let codes: [UInt16: KeyStroke.Key] = [
            114: .insert, 105: .function(13), 107: .function(14), 113: .function(15),
            106: .function(16), 64: .function(17), 79: .function(18), 80: .function(19),
            90: .function(20),
        ]
        for (code, key) in codes {
            XCTAssertEqual(KeyStroke(keyDown("\u{F746}", keyCode: code))?.key, key, "keyCode \(code)")
        }
    }
}
