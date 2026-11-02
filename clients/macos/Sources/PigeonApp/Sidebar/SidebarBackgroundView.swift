import AppKit

/// The sidebar's own surface: it fills with harp's `SIDEBAR` token and runs up under
/// the transparent titlebar, so the traffic lights sit on it.
///
/// Drawn rather than set as a layer color, because a dynamic `NSColor` resolves at
/// draw time — switching the system appearance repaints without extra bookkeeping.
final class SidebarBackgroundView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        SidebarTheme.sidebar.setFill()
        dirtyRect.fill()
    }
}
