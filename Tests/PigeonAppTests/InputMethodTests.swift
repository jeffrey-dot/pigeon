import AppKit
import XCTest
@testable import PigeonApp

/// Input-method composition: what the pane draws while composing, what it sends on commit,
/// and which keys bypass the input method.
final class InputMethodTests: XCTestCase {
    private func grid(cols: Int, rows: Int) -> [PaneView.Cell] {
        Array(repeating: .blank, count: cols * rows)
    }

    func testPreeditIsDrawnAtTheCursorWithWideCharacters() throws {
        let preedit = Preedit(text: "中a", caret: 1)
        let cursor = PaneView.Cursor(x: 2, y: 1, visible: true)
        let result = try XCTUnwrap(preedit.overlay(cells: grid(cols: 10, rows: 3), cursor: cursor, cols: 10, rows: 3))
        let row = Array(result.cells[10..<20])
        XCTAssertEqual(row[2].codepoint, 0x4e2d)
        XCTAssertEqual(row[2].width, 2)
        XCTAssertEqual(row[3].width, 0)
        XCTAssertEqual(row[4].codepoint, UInt32(UInt8(ascii: "a")))
        XCTAssertNotEqual(row[2].attrs & CellAttrs.underline, 0)
        // The caret after `中` (one UTF-16 unit) sits on the column after its two cells.
        XCTAssertEqual(result.cursor.x, 4)
        XCTAssertEqual(result.row, 1)
    }

    func testPreeditNearTheRightEdgeSlidesLeft() throws {
        let place = try XCTUnwrap(Preedit(text: "abcd", caret: 4)
            .placement(cursor: .init(x: 8, y: 0, visible: true), cols: 10, rows: 1))
        XCTAssertEqual(place.column, 6)
        XCTAssertEqual(place.caretColumn, 9)
    }

    func testPreeditWiderThanTheRowKeepsItsTailAndNoHalfGlyph() throws {
        let place = try XCTUnwrap(Preedit(text: "中文字", caret: 3)
            .placement(cursor: .init(x: 0, y: 0, visible: true), cols: 5, rows: 1))
        XCTAssertEqual(place.cells.count, 5)
        XCTAssertNotEqual(place.cells[0].width, 0)
        XCTAssertEqual(place.cells.last?.width, 0)
    }

    func testMalformedGeometryDrawsNothing() {
        let preedit = Preedit(text: "ni", caret: 2)
        XCTAssertNil(preedit.placement(cursor: .init(x: 0, y: 0, visible: true), cols: 0, rows: 0))
        XCTAssertNil(Preedit(text: "", caret: 0)
            .placement(cursor: .init(x: 0, y: 0, visible: true), cols: 10, rows: 1))
        // A grid that does not match its dimensions (a resize raced the frame).
        XCTAssertNil(preedit.overlay(cells: grid(cols: 3, rows: 1), cursor: .init(x: 0, y: 0, visible: true), cols: 10, rows: 2))
        // A cursor off the grid is clamped, not trusted.
        let place = preedit.placement(cursor: .init(x: 99, y: -4, visible: true), cols: 10, rows: 2)
        XCTAssertEqual(place?.row, 0)
        XCTAssertEqual(place?.column, 8)
    }

    func testComposedTextReachesTheHostOnlyOnCommit() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var sent: [[UInt8]] = []
        pane.onInput = { sent.append($0) }
        pane.setMarkedText("ni", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertTrue(pane.hasMarkedText())
        XCTAssertEqual(pane.markedRange(), NSRange(location: 0, length: 2))
        XCTAssertTrue(sent.isEmpty)
        pane.insertText("你", replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertFalse(pane.hasMarkedText())
        XCTAssertEqual(sent, [Array("你".utf8)])
    }

    func testUnmarkKeepsWhatWasTypedAndEmptyMarkedTextCancels() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
        var sent: [[UInt8]] = []
        pane.onInput = { sent.append($0) }
        pane.setMarkedText(NSAttributedString(string: "e"), selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        pane.unmarkText()
        XCTAssertEqual(sent, [Array("e".utf8)])
        pane.setMarkedText("k", selectedRange: NSRange(location: 1, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        pane.setMarkedText("", selectedRange: NSRange(location: 0, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertFalse(pane.hasMarkedText())
        XCTAssertEqual(sent.count, 1)
    }

    func testOptionGoesToTheInputMethodOnlyWhenNotMeta() {
        let pane = PaneView(frame: .zero)
        let event = NSEvent()
        let option = KeyStroke(key: .character("e"), option: true)
        XCTAssertFalse(pane.routesThroughInputMethod(event, stroke: option))
        pane.optionAsMeta = false
        XCTAssertTrue(pane.routesThroughInputMethod(event, stroke: option))
        XCTAssertFalse(pane.routesThroughInputMethod(event, stroke: KeyStroke(key: .character("c"), control: true)))
        XCTAssertFalse(pane.routesThroughInputMethod(event, stroke: KeyStroke(key: .up)))
        XCTAssertTrue(pane.routesThroughInputMethod(event, stroke: KeyStroke(key: .character("a"))))
    }

    func testWhileComposingEveryKeyBelongsToTheInputMethod() {
        let pane = PaneView(frame: .zero)
        pane.setMarkedText("ni", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        XCTAssertTrue(pane.routesThroughInputMethod(NSEvent(), stroke: KeyStroke(key: .enter)))
        XCTAssertTrue(pane.routesThroughInputMethod(NSEvent(), stroke: KeyStroke(key: .escape)))
    }

    func testSettingsDefaultAndTolerateOldOrBrokenFiles() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let url = directory.appendingPathComponent("settings.json")
        let store = TerminalSettingsStore(url: url)
        XCTAssertTrue(store.load().optionAsMeta)
        store.save(TerminalSettings(optionAsMeta: false))
        XCTAssertFalse(store.load().optionAsMeta)
        try Data("{}".utf8).write(to: url)
        XCTAssertTrue(store.load().optionAsMeta)
        try Data("not json".utf8).write(to: url)
        XCTAssertTrue(store.load().optionAsMeta)
        try? FileManager.default.removeItem(at: directory)
    }

    func testCharacterWidths() {
        XCTAssertEqual(CharacterWidth.of("a"), 1)
        XCTAssertEqual(CharacterWidth.of("中"), 2)
        XCTAssertEqual(CharacterWidth.of("ア"), 2)
        XCTAssertEqual(CharacterWidth.of("😀"), 2)
        XCTAssertEqual(CharacterWidth.of("\u{0301}"), 0)
        XCTAssertEqual(CharacterWidth.of("\u{07}"), 0)
    }
}
