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
}
