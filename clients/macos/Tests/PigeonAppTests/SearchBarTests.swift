import AppKit
import XCTest
@testable import PigeonApp

/// Requires macOS: these exercise AppKit delegate routing, not the terminal encoder.
final class SearchBarTests: XCTestCase {
    func testSearchFieldConsumesNavigationAndEscapeButLeavesTextEditingToAppKit() {
        _ = NSApplication.shared
        let bar = SearchBarView(frame: NSRect(x: 0, y: 0, width: 800, height: 64))
        let editor = NSTextView()
        var navigation: [Int] = []
        var closed = false
        bar.onNavigate = { navigation.append($0) }
        bar.onClose = { closed = true }
        XCTAssertTrue(bar.control(bar.field, textView: editor, doCommandBy: #selector(NSResponder.moveDown(_:))))
        XCTAssertTrue(bar.control(bar.field, textView: editor, doCommandBy: #selector(NSResponder.moveUp(_:))))
        XCTAssertEqual(navigation, [1, -1])
        XCTAssertFalse(bar.control(bar.field, textView: editor, doCommandBy: #selector(NSResponder.deleteBackward(_:))))
        XCTAssertTrue(bar.control(bar.field, textView: editor, doCommandBy: #selector(NSResponder.cancelOperation(_:))))
        XCTAssertTrue(closed)
    }

    func testLayoutRefreshDoesNotStealSearchFieldFocus() {
        _ = NSApplication.shared
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 500),
                              styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        let container = SplitContainerView(frame: NSRect(x: 0, y: 64, width: 800, height: 436))
        let bar = SearchBarView(frame: NSRect(x: 0, y: 0, width: 800, height: 64))
        content.addSubview(container); content.addSubview(bar)
        window.contentView = content
        container.preservesKeyboardFocus = { true }
        let model = LayoutModel(panes: [.init(id: 1, rect: CGRect(x: 0, y: 0, width: 1, height: 1))],
                                dividers: [], focus: 1)
        container.apply(model)
        bar.focus()
        let responder = window.firstResponder
        XCTAssertNotNil(responder)
        container.apply(model)
        XCTAssertTrue(window.firstResponder === responder)
    }
}
