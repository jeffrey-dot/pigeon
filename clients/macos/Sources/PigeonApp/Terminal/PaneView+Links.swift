import AppKit

extension PaneView {
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let linkTracking { removeTrackingArea(linkTracking) }
        let area = NSTrackingArea(
            rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self, userInfo: nil)
        addTrackingArea(area)
        linkTracking = area
    }

    override func mouseMoved(with event: NSEvent) {
        updateLinkHover(at: event.locationInWindow, command: event.modifierFlags.contains(.command))
    }

    override func mouseEntered(with event: NSEvent) { mouseMoved(with: event) }

    override func mouseExited(with event: NSEvent) {
        toolTip = nil
        NSCursor.iBeam.set()
    }

    func refreshLinkHover() {
        guard let window else { return }
        updateLinkHover(at: window.mouseLocationOutsideOfEventStream,
                        command: NSEvent.modifierFlags.contains(.command))
    }

    private func link(at location: NSPoint) -> (URL, TerminalSelection.Point)? {
        let local = convert(location, from: nil)
        guard bounds.contains(local), cellSize.width > 0, cellSize.height > 0 else { return nil }
        let column = Int(local.x / cellSize.width)
        let row = Int((bounds.height - local.y) / cellSize.height)
        guard column >= 0, column < cols, row >= 0, row < rows,
              cells.indices.contains(row * cols + column),
              let url = TerminalLinks.openingURL(cells[row * cols + column].hyperlink)
        else { return nil }
        return (url, TerminalSelection.Point(row: row, column: column))
    }

    private func updateLinkHover(at location: NSPoint, command: Bool) {
        guard bounds.contains(convert(location, from: nil)) else { toolTip = nil; return }
        let target = link(at: location)?.0
        toolTip = target.map { "Command-click to open: \($0.absoluteString)" }
        if target != nil && command { NSCursor.pointingHand.set() }
        else { NSCursor.iBeam.set() }
    }

    func beginLinkClick(_ event: NSEvent) -> Bool {
        guard event.modifierFlags.contains(.command), let (url, point) = link(at: event.locationInWindow)
        else { return false }
        linkGesture = true
        pressedLink = TerminalLinkPress(url: url, point: point)
        return true
    }

    func finishLinkClick(_ event: NSEvent) -> Bool {
        guard linkGesture else { return false }
        defer { linkGesture = false; pressedLink = nil }
        guard event.modifierFlags.contains(.command), let pressedLink,
              let (url, point) = link(at: event.locationInWindow),
              url == pressedLink.url, point == pressedLink.point
        else { return true }
        NSWorkspace.shared.open(url)
        return true
    }
}
