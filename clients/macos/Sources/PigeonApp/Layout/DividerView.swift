import AppKit

/// A hairline in the card's palette (`SidebarTheme.divider`, harp's `DIVIDER`).
/// `NSBox(.separator)` draws a system-tinted 1pt line, which is neither the color nor the
/// 0.5pt weight harp uses under the card header.
final class DividerView: NSView {
    override var isOpaque: Bool { false }

    override func draw(_ dirtyRect: NSRect) {
        SidebarTheme.divider.setFill()
        dirtyRect.fill()
    }
}
