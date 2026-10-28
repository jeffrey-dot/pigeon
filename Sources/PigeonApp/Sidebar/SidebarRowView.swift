import AppKit

/// The row pill from harp (`docs/ui.md` §7.1/§7.2): 8pt radius, 0.5pt edge, a filled
/// white (dark: lifted grey) surface when selected, a faint wash under the pointer.
///
/// `NSTableView.selectionHighlightStyle` is `.none`; this view owns the whole look so
/// the sidebar does not get the accent-colored system highlight.
final class SidebarRowView: NSTableRowView {
    private var hovered = false
    private var tracking: NSTrackingArea?

    override var isEmphasized: Bool {
        didSet { needsDisplay = true }
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

    override func drawBackground(in dirtyRect: NSRect) {
        guard isSelected || hovered else { return }
        // Rows sit inside the list's 10pt side padding, so the pill keeps that much air.
        let pill = bounds.insetBy(dx: SidebarMetrics.listSidePadding, dy: 0).insetBy(dx: 0.25, dy: 0.25)
        let path = NSBezierPath(roundedRect: pill, xRadius: SidebarMetrics.rowRadius, yRadius: SidebarMetrics.rowRadius)
        let fill = isSelected
            ? (isEmphasized ? SidebarTheme.selected : SidebarTheme.quietSelected)
            : SidebarTheme.hover
        if isSelected && isEmphasized {
            NSGraphicsContext.saveGraphicsState()
            let shadow = NSShadow()
            shadow.shadowColor = SidebarTheme.selectedEdge
            shadow.shadowBlurRadius = 2
            shadow.shadowOffset = NSSize(width: 0, height: -1)
            shadow.set()
            fill.setFill()
            path.fill()
            NSGraphicsContext.restoreGraphicsState()
        }
        fill.setFill()
        path.fill()
        if isSelected {
            SidebarTheme.selectedEdge.setStroke()
            path.lineWidth = SidebarMetrics.dividerWidth
            path.stroke()
        }
    }

    override func drawSelection(in dirtyRect: NSRect) {}
}
