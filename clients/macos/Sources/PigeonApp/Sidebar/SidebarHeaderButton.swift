import AppKit

/// A header chip from harp (`VT:10146-10162`): 28×28, 7pt rounded hover fill, glyph only.
/// `NSButton` cannot do the hover surface, so the pill is drawn underneath the image.
final class SidebarHeaderButton: NSButton {
    private var tracking: NSTrackingArea?
    private var hovered = false

    override init(frame: NSRect) {
        super.init(frame: frame)
        isBordered = false
        wantsLayer = false
        updateTrackingAreas()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        isBordered = false
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) {
        hovered = true
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        hovered = false
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        if hovered {
            SidebarTheme.hover.setFill()
            NSBezierPath(
                roundedRect: bounds,
                xRadius: SidebarMetrics.headerButtonRadius,
                yRadius: SidebarMetrics.headerButtonRadius
            ).fill()
        }
        super.draw(dirtyRect)
    }
}
