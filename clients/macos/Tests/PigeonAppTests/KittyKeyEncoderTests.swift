import XCTest
@testable import PigeonApp

/// The kitty keyboard protocol's encoding table, flag by flag
/// (https://sw.kovidgoyal.net/kitty/keyboard-protocol/).
final class KittyKeyEncoderTests: XCTestCase {
    private func modes(_ flags: UInt32) -> UInt32 { flags << TerminalModes.kittyShift }

    private func encode(_ stroke: KeyStroke, _ flags: UInt32, extra: UInt32 = 0) -> String? {
        KeyEncoder.encode(stroke, modes: modes(flags) | extra).map { String(decoding: $0, as: UTF8.self) }
    }

    private func char(
        _ character: Character, control: Bool = false, option: Bool = false, shift: Bool = false,
        action: KeyStroke.Action = .press
    ) -> KeyStroke {
        KeyStroke(key: .character(character), control: control, option: option, shift: shift, action: action)
    }

    // MARK: Flags reach the encoder from the modes word

    func testFlagsComeFromBits10To14() {
        XCTAssertEqual(KittyKeyEncoder.Flags(modes: 1 << 10), .disambiguate)
        XCTAssertEqual(KittyKeyEncoder.Flags(modes: 0x1f << 10).rawValue, 31)
        // Edge: neighbouring mode bits (cursor blink, anything above 14) are not kitty flags.
        XCTAssertTrue(KittyKeyEncoder.Flags(modes: TerminalModes.cursorBlink | 1 << 15).isEmpty)
    }

    func testWithoutFlagsKeysStayLegacy() {
        XCTAssertEqual(encode(char("c", control: true), 0), "\u{3}")
        XCTAssertEqual(encode(KeyStroke(key: .escape), 0), "\u{1b}")
        XCTAssertNil(encode(char("a", action: .release), 0), "legacy never sends releases")
        XCTAssertNil(encode(KeyStroke(key: .modifier(.leftShift)), 0))
    }

    // MARK: Flag 1, disambiguate

    func testDisambiguateTurnsChordsIntoCSIu() {
        XCTAssertEqual(encode(char("c", control: true), 1), "\u{1b}[99;5u")
        XCTAssertEqual(encode(char("a", option: true), 1), "\u{1b}[97;3u")
        XCTAssertEqual(encode(char("a", control: true, option: true), 1), "\u{1b}[97;7u")
        // The code is the unshifted key: Ctrl+Shift+A is 97, never 65.
        XCTAssertEqual(encode(char("A", control: true, shift: true), 1), "\u{1b}[97;6u")
        XCTAssertEqual(encode(char("i", control: true), 1), "\u{1b}[105;5u", "Ctrl+I is not Tab")
        XCTAssertEqual(encode(KeyStroke(key: .escape), 1), "\u{1b}[27u")
        XCTAssertEqual(encode(KeyStroke(key: .escape, option: true), 1), "\u{1b}[27;3u")
    }

    func testDisambiguateKeepsTextAsText() {
        XCTAssertEqual(encode(char("a"), 1), "a")
        XCTAssertEqual(encode(char("A", shift: true), 1), "A")
        XCTAssertEqual(encode(char("你"), 1), "你")
    }

    func testModifiedEnterTabAndBackspaceBecomeDistinguishable() {
        XCTAssertEqual(encode(KeyStroke(key: .enter, shift: true), 1), "\u{1b}[13;2u")
        XCTAssertEqual(encode(KeyStroke(key: .enter, control: true), 1), "\u{1b}[13;5u")
        XCTAssertEqual(encode(KeyStroke(key: .tab, control: true), 1), "\u{1b}[9;5u")
        XCTAssertEqual(encode(KeyStroke(key: .tab, shift: true), 1), "\u{1b}[9;2u")
        XCTAssertEqual(encode(KeyStroke(key: .backspace, option: true), 1), "\u{1b}[127;3u")
        // Unmodified, they keep legacy bytes so `reset⏎` works after a crash.
        XCTAssertEqual(encode(KeyStroke(key: .enter), 1), "\r")
        XCTAssertEqual(encode(KeyStroke(key: .tab), 1), "\t")
        XCTAssertEqual(encode(KeyStroke(key: .backspace), 1), "\u{7f}")
        // Edge: caps lock alone does not count as a modifier here.
        XCTAssertEqual(encode(KeyStroke(key: .enter, capsLock: true), 1), "\r")
    }

    func testFunctionalKeysUseTheirCSIForms() {
        XCTAssertEqual(encode(KeyStroke(key: .up), 1), "\u{1b}[A")
        XCTAssertEqual(encode(KeyStroke(key: .up), 1, extra: TerminalModes.appCursor), "\u{1b}OA",
                       "an unmodified arrow still honours DECCKM")
        XCTAssertEqual(encode(KeyStroke(key: .left, control: true), 1), "\u{1b}[1;5D")
        XCTAssertEqual(encode(KeyStroke(key: .insert, shift: true), 1), "\u{1b}[2;2~")
        XCTAssertEqual(encode(KeyStroke(key: .function(3), control: true), 1), "\u{1b}[13;5~",
                       "F3 is never CSI R (a cursor position report)")
        XCTAssertEqual(encode(KeyStroke(key: .function(1), shift: true), 1), "\u{1b}[1;2P")
        XCTAssertEqual(encode(KeyStroke(key: .function(13)), 1), "\u{1b}[57376u")
        XCTAssertEqual(encode(KeyStroke(key: .function(20), control: true), 1), "\u{1b}[57383;5u")
        XCTAssertNil(encode(KeyStroke(key: .delete, command: true), 1), "Command chords belong to the app")
    }

    func testNonLatinChordReportsTheLayoutCharacter() {
        let stroke = KeyStroke(key: .character("с"), control: true, base: "c")
        XCTAssertEqual(encode(stroke, 1), "\u{1b}[1089;5u")
    }

    // MARK: Flag 2, event types

    func testEventTypesReportRepeatAndRelease() {
        let flags: UInt32 = 1 | 2
        XCTAssertEqual(encode(char("c", control: true, action: .repeated), flags), "\u{1b}[99;5:2u")
        XCTAssertEqual(encode(char("c", control: true, action: .release), flags), "\u{1b}[99;5:3u")
        XCTAssertEqual(encode(KeyStroke(key: .escape, action: .release), flags), "\u{1b}[27;1:3u",
                       "no modifiers still writes 1 before the event type")
        XCTAssertEqual(encode(KeyStroke(key: .up, action: .release), flags), "\u{1b}[1;1:3A")
        XCTAssertEqual(encode(KeyStroke(key: .pageDown, action: .repeated), flags), "\u{1b}[6;1:2~")
        XCTAssertEqual(encode(KeyStroke(key: .up, action: .press), flags), "\u{1b}[A", "press is the default")
    }

    func testTextKeysAndEnterReportNoReleaseWithoutAllKeys() {
        let flags: UInt32 = 1 | 2
        XCTAssertNil(encode(char("a", action: .release), flags))
        XCTAssertEqual(encode(char("a", action: .repeated), flags), "a")
        XCTAssertNil(encode(KeyStroke(key: .enter, action: .release), flags))
        XCTAssertNil(encode(KeyStroke(key: .backspace, control: true, action: .release), flags))
        // Edge: releases need flag 2 even with disambiguate on.
        XCTAssertNil(encode(KeyStroke(key: .escape, action: .release), 1))
    }

    // MARK: Flag 4, alternate keys

    func testAlternateKeysAddShiftedAndBaseLayoutKeys() {
        let flags: UInt32 = 1 | 4
        XCTAssertEqual(encode(char("A", control: true, shift: true), flags), "\u{1b}[97:65;6u")
        var bang = char("!", control: true, shift: true)
        bang.unshifted = "1"
        XCTAssertEqual(encode(bang, flags), "\u{1b}[49:33;6u")
        let russian = KeyStroke(key: .character("с"), control: true, base: "c")
        XCTAssertEqual(encode(russian, flags), "\u{1b}[1089::99;5u")
        let russianShift = KeyStroke(key: .character("С"), control: true, shift: true, base: "c")
        XCTAssertEqual(encode(russianShift, flags), "\u{1b}[1089:1057:99;6u")
        // Edge: no shift and an ASCII key → nothing to add.
        XCTAssertEqual(encode(char("c", control: true), flags), "\u{1b}[99;5u")
        // The flag only changes keys that are already escape codes.
        XCTAssertEqual(encode(char("A", shift: true), flags), "A")
    }

    // MARK: Flag 8, all keys as escape codes

    func testAllKeysReportsTextKeysAndEnter() {
        XCTAssertEqual(encode(char("a"), 8), "\u{1b}[97u")
        XCTAssertEqual(encode(char("A", shift: true), 8), "\u{1b}[97;2u")
        XCTAssertEqual(encode(KeyStroke(key: .enter), 8), "\u{1b}[13u")
        XCTAssertEqual(encode(KeyStroke(key: .tab), 8), "\u{1b}[9u")
        XCTAssertEqual(encode(KeyStroke(key: .backspace), 8), "\u{1b}[127u")
        XCTAssertEqual(encode(KeyStroke(key: .up), 8, extra: TerminalModes.appCursor), "\u{1b}[A")
        XCTAssertEqual(encode(KeyStroke(key: .enter, action: .release), 8 | 2), "\u{1b}[13;1:3u")
        XCTAssertEqual(encode(char("a", action: .release), 8 | 2), "\u{1b}[97;1:3u")
    }

    func testAllKeysReportsLockModifiersAndModifierKeys() {
        XCTAssertEqual(encode(KeyStroke(key: .character("a"), capsLock: true), 8), "\u{1b}[97;65u")
        XCTAssertEqual(encode(KeyStroke(key: .character("a"), control: true, capsLock: true), 1),
                       "\u{1b}[97;5u", "disambiguate alone omits locks on text keys")
        XCTAssertEqual(encode(KeyStroke(key: .modifier(.leftShift), shift: true), 8), "\u{1b}[57441;2u")
        XCTAssertEqual(encode(KeyStroke(key: .modifier(.rightControl), action: .release), 8 | 2),
                       "\u{1b}[57448;1:3u")
        XCTAssertEqual(encode(KeyStroke(key: .modifier(.leftCommand), command: true), 8), "\u{1b}[57444;9u")
        XCTAssertNil(encode(KeyStroke(key: .modifier(.leftShift), shift: true), 1),
                     "modifier keys alone only report under flag 8")
    }

    func testKeypadKeysGetTheirOwnCodes() {
        XCTAssertEqual(encode(KeyStroke(key: .character("5"), keypad: true), 8), "\u{1b}[57404u")
        XCTAssertEqual(encode(KeyStroke(key: .enter, keypad: true), 8), "\u{1b}[57414u")
        XCTAssertEqual(encode(KeyStroke(key: .enter, shift: true, keypad: true), 1), "\u{1b}[57414;2u")
        // Under disambiguate alone keypad digits still type, even in DECKPAM.
        XCTAssertEqual(encode(KeyStroke(key: .character("5"), keypad: true), 1), "5")
    }

    // MARK: Flag 16, associated text

    func testAssociatedTextRidesInTheThirdField() {
        let flags: UInt32 = 8 | 16
        XCTAssertEqual(encode(char("a"), flags), "\u{1b}[97;;97u")
        XCTAssertEqual(encode(char("A", shift: true), flags), "\u{1b}[97;2;65u")
        var composed = KeyStroke(key: .character("a"), option: true)
        composed.text = "å"
        XCTAssertEqual(encode(composed, flags), "\u{1b}[97;3;229u")
        var multi = char("a")
        multi.text = "ab"
        XCTAssertEqual(encode(multi, flags), "\u{1b}[97;;97:98u")
    }

    /// Malformed text: control characters never travel; a Ctrl chord, a release or a text
    /// that is all controls carries no third field at all.
    func testAssociatedTextDropsControlsAndChords() {
        let flags: UInt32 = 8 | 16 | 2
        var withControls = char("a")
        withControls.text = "a\u{1b}\u{7f}\u{85}b"
        XCTAssertEqual(encode(withControls, flags), "\u{1b}[97;;97:98u")
        var escapeSequence = char("a")
        escapeSequence.text = "\u{1b}[31m"
        XCTAssertEqual(encode(escapeSequence, flags), "\u{1b}[97;;91:51:49:109u")
        var bell = char("a")
        bell.text = "\u{7}"
        XCTAssertEqual(encode(bell, flags), "\u{1b}[97u")
        XCTAssertEqual(encode(char("c", control: true), flags), "\u{1b}[99;5u")
        XCTAssertEqual(encode(char("a", action: .release), flags), "\u{1b}[97;1:3u")
        XCTAssertEqual(encode(KeyStroke(key: .enter), flags), "\u{1b}[13u", "Enter's CR is a control")
    }
}
