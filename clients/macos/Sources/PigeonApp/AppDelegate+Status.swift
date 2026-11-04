import AppKit

extension AppDelegate {
    /// A selected session on an occluded/inactive window has not been seen.
    var visibleStatusPanes: Set<UInt32> {
        guard swappedFrom == nil, displayedHost == activeHost, displayedSession == selectedSession,
              NSApp.isActive, window?.isKeyWindow == true,
              window?.occlusionState.contains(.visible) == true,
              let container else { return [] }
        return Set(container.model.panes.compactMap { pane in
            guard haveSnapshot.contains(pane.id),
                  !SplitContainerView.hides(pane: pane.id, zoom: container.model.zoom)
            else { return nil }
            return pane.id
        })
    }

    func markVisibleStatusesSeen() {
        guard let session = selectedSession else { return }
        let panes = visibleStatusPanes
        guard !panes.isEmpty else { return }
        let effects = SessionReducer.markSeen(panes, in: session, rows: &rows, host: activeHost)
        applyStatusEffects(effects)
        if !effects.isEmpty { refreshSidebar() }
    }

    func applyStatusEffects(_ effects: [SessionReducer.Effect]) {
        var changed = false
        for effect in effects {
            switch effect {
            case let .ack(pane, seq): sendAck(pane, seq)
            case let .saveUnread(ref, count):
                state.unread[ref.key] = count
                changed = true
            }
        }
        if changed { store.save(state) }
    }

    @objc func statusWindowBecameVisible(_ notification: Notification) {
        markVisibleStatusesSeen()
    }
}
