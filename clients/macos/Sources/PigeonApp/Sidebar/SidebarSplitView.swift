import AppKit

/// The card already draws its edge. Keep the resize hit target without a second system line.
final class SidebarSplitView: NSSplitView {
    override func drawDivider(in rect: NSRect) {
        SidebarTheme.sidebar.setFill()
        rect.fill()
    }
}
