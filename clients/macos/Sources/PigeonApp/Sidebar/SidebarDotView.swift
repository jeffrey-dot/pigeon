import AppKit

/// The unread dot. Drawn instead of a layer color: `NSColor.cgColor` is a snapshot taken
/// for the appearance at that moment, so a layer would keep the old palette after a
/// system appearance switch.
final class SidebarDotView: NSView {
    override var isOpaque: Bool { false }

    override func draw(_ dirtyRect: NSRect) {
        SidebarTheme.iconStrong.setFill()
        NSBezierPath(ovalIn: bounds.insetBy(dx: 0.5, dy: 0.5)).fill()
    }
}
