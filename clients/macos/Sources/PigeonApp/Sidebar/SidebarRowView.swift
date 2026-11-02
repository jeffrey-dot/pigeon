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

    override var isSelected: Bool {
        didSet { if isSelected != oldValue { fadeNextDraw() } }
    }

    /// Selection and hover changes cross-fade over `SidebarMetrics.motion` instead of snapping.
    /// The transition animates the layer's next redraw, so the drawing code stays as is.
    private func fadeNextDraw() {
        wantsLayer = true
        needsDisplay = true
        guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else { return }
        let fade = CATransition()
        fade.type = .fade
        fade.duration = SidebarMetrics.motion
        fade.timingFunction = CAMediaTimingFunction(name: .easeOut)
        layer?.add(fade, forKey: "fade")
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
        fadeNextDraw()
    }

    override func mouseExited(with event: NSEvent) {
        hovered = false
        fadeNextDraw()
    }

    override func drawBackground(in dirtyRect: NSRect) {
        guard isSelected || hovered else { return }
        // Rows sit inside the list's 10pt side padding, so the pill keeps that much air.
        let pill = bounds
            .insetBy(dx: SidebarMetrics.listSidePadding, dy: 0)
            .insetBy(dx: SidebarMetrics.rowPillInset, dy: SidebarMetrics.rowPillInset)
        let path = NSBezierPath(roundedRect: pill, xRadius: SidebarMetrics.rowRadius, yRadius: SidebarMetrics.rowRadius)
        // harp's `SelectedQuiet` is for a window that is not key, not for "this table is not
        // first responder": the terminal pane holds focus in pigeon, so `isEmphasized` alone
        // would paint the quiet surface in a perfectly active window.
        let key = window?.isKeyWindow ?? false
        let fill = isSelected
            ? (key ? SidebarTheme.selected : SidebarTheme.quietSelected)
            : SidebarTheme.hover
        if isSelected && isEmphasized {
            NSGraphicsContext.saveGraphicsState()
            let shadow = NSShadow()
            shadow.shadowColor = SidebarTheme.selectedShadow
            shadow.shadowBlurRadius = SidebarMetrics.selectedShadowBlur
            shadow.shadowOffset = NSSize(width: 0, height: SidebarMetrics.selectedShadowOffsetY)
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
