import XCTest
@testable import PigeonApp

final class MouseEncoderTests: XCTestCase {
    private let sgrClicks = TerminalModes.mouseClick | TerminalModes.mouseSGR
    private func text(_ bytes: [UInt8]?) -> String? { bytes.map { String(decoding: $0, as: UTF8.self) } }

    func testNothingIsReportedUnlessTheProgramAsked() {
        XCTAssertNil(MouseEncoder.encode(.left, .press, column: 0, row: 0, modes: 0))
        XCTAssertNil(MouseEncoder.encode(.left, .press, column: 0, row: 0, modes: TerminalModes.mouseSGR),
                     "the format bit alone is not a request")
    }

    func testSGRClicksAreOneBasedAndReleaseKeepsTheButton() {
        XCTAssertEqual(text(MouseEncoder.encode(.left, .press, column: 4, row: 2, modes: sgrClicks)), "\u{1b}[<0;5;3M")
        XCTAssertEqual(text(MouseEncoder.encode(.left, .release, column: 4, row: 2, modes: sgrClicks)), "\u{1b}[<0;5;3m")
        XCTAssertEqual(text(MouseEncoder.encode(.right, .press, column: 0, row: 0, modes: sgrClicks)), "\u{1b}[<2;1;1M")
    }

    func testTheWheelAndModifiers() {
        XCTAssertEqual(text(MouseEncoder.encode(.wheelUp, .press, column: 0, row: 0, modes: sgrClicks)), "\u{1b}[<64;1;1M")
        XCTAssertEqual(text(MouseEncoder.encode(
            .left, .press, column: 0, row: 0, modifiers: .init(shift: false, option: false, control: true),
            modes: sgrClicks)), "\u{1b}[<16;1;1M")
    }

    func testDragsAndMotionNeedTheirOwnModes() {
        XCTAssertNil(MouseEncoder.encode(.left, .drag, column: 1, row: 1, modes: sgrClicks))
        let drag = sgrClicks | TerminalModes.mouseDrag
        XCTAssertEqual(text(MouseEncoder.encode(.left, .drag, column: 1, row: 1, modes: drag)), "\u{1b}[<32;2;2M")
        XCTAssertNil(MouseEncoder.encode(.none, .move, column: 1, row: 1, modes: drag))
        let motion = sgrClicks | TerminalModes.mouseMotion
        XCTAssertEqual(text(MouseEncoder.encode(.none, .move, column: 1, row: 1, modes: motion)), "\u{1b}[<35;2;2M")
    }

    func testX10ReportsAreBytesAndGiveUpPastColumn223() {
        let x10 = TerminalModes.mouseClick
        XCTAssertEqual(MouseEncoder.encode(.left, .press, column: 0, row: 0, modes: x10), [0x1b, 0x5b, 0x4d, 32, 33, 33])
        XCTAssertEqual(MouseEncoder.encode(.left, .release, column: 0, row: 0, modes: x10), [0x1b, 0x5b, 0x4d, 35, 33, 33])
        XCTAssertNil(MouseEncoder.encode(.left, .press, column: 300, row: 0, modes: x10), "no byte can say 301")
    }
}
