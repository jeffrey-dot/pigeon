import XCTest
@testable import PigeonApp

final class CursorBlinkTests: XCTestCase {
    private let start = Date(timeIntervalSince1970: 1_000)

    func testSolidWhileTypingThenAlternates() {
        let blink = CursorBlink(now: start)
        XCTAssertTrue(blink.isLit(at: start.addingTimeInterval(0.2)), "solid right after a key")
        let first = start.addingTimeInterval(CursorBlink.rest + 0.1)
        let second = first.addingTimeInterval(CursorBlink.period)
        XCTAssertNotEqual(blink.isLit(at: first), blink.isLit(at: second))
        XCTAssertEqual(blink.isLit(at: first), blink.isLit(at: second.addingTimeInterval(CursorBlink.period)))
    }

    func testTypingResetsThePhaseAndABackwardsClockStaysSolid() {
        var blink = CursorBlink(now: start)
        let later = start.addingTimeInterval(10)
        blink.typed(at: later)
        XCTAssertTrue(blink.isLit(at: later.addingTimeInterval(0.1)))
        XCTAssertTrue(blink.isLit(at: start), "time before the last key")
    }

    func testThePaneBlinksOnlyWhenAskedAndFocused() {
        let pane = PaneView(frame: NSRect(x: 0, y: 0, width: 200, height: 100))
        pane.modes = TerminalModes.cursorBlink
        XCTAssertNil(pane.blinkTimer, "no window, nobody to blink for")
        pane.modes = 0
        XCTAssertNil(pane.blinkTimer)
    }
}
