import AppKit
import PigeonCore

extension AppDelegate {
    /// A selected session on an occluded/inactive window has not been seen.
    var visibleStatusPanes: Set<UInt32> {
        guard swappedFrom == nil, let container else { return [] }
        let presented = Set(container.model.panes.compactMap { pane -> UInt32? in
            guard haveSnapshot.contains(pane.id),
                  connection.receivedScreen.contains(pane.id),
                  let view = container.paneView(pane.id), !view.cells.isEmpty,
                  view.metal != nil, view.bounds.width > 0, view.bounds.height > 0,
                  view.scrollOffset == 0, !view.isHiddenOrHasHiddenAncestor,
                  !SplitContainerView.hides(pane: pane.id, zoom: container.model.zoom)
            else { return nil }
            return pane.id
        })
        return StatusVisibility(
            activeHost: activeHost, displayedHost: displayedHost, selectedSession: selectedSession,
            displayedSession: displayedSession, ready: connection.ready && clients[activeHost] != nil,
            windowFocused: NSApp.isActive && window?.isKeyWindow == true,
            windowVisible: window?.occlusionState.contains(.visible) == true,
            presentedPanes: presented).panes(for: activeHost, session: selectedSession)
    }

    func markVisibleStatusesSeen() {
        // A batch may replace a terminal report before AppKit ever presents it. Inspect
        // the current reports after the batch, alongside queued screen updates.
        guard !statusReadScheduled else { return }
        statusReadScheduled = true
        RunLoop.main.perform(inModes: [.common]) { [weak self] in
            guard let self else { return }
            self.statusReadScheduled = false
            self.acknowledgeVisibleStatuses()
        }
    }

    private func acknowledgeVisibleStatuses() {
        guard let session = selectedSession, let instance = connection.instance else { return }
        let panes = visibleStatusPanes
        guard !panes.isEmpty else { return }
        let effects = SessionReducer.markSeen(
            panes, in: session, rows: &rows, host: activeHost, instance: instance)
        applyStatusEffects(effects)
        if !effects.isEmpty { refreshSidebar() }
    }

    func applyStatusEffects(_ effects: [SessionReducer.Effect]) {
        let previous = state
        // Durable seen identities must be written before the ack can leave the process.
        for effect in effects {
            if case let .saveStatus(ref, instance, receipts) = effect {
                state.save(receipts, for: ref, instance: instance)
            }
        }
        if state != previous { store.save(state) }
        for case let .ack(target) in effects {
            guard connections[target.host]?.accepts(target, host: target.host) == true,
                  let client = clients[target.host], let message = Wire.make(PG_MSG_ACK_STATUS)
            else { continue }
            Wire.set(message, u32: "pane_id", target.pane)
            Wire.set(message, u32: "seq", target.record.seq)
            client.send(message)
            Wire.free(message)
        }
    }

    func applyStatus(_ message: OpaquePointer) {
        guard let instance = connection.instance else { return }
        let report = SessionReducer.Report(
            pane: Wire.u32(message, "pane_id"),
            status: SidebarModel.status(from: Wire.string(message, "state")),
            seq: Wire.u32(message, "seq"),
            agent: AgentInfo(message: message), record: Wire.string(message, "id"))
        let effects = SessionReducer.apply(
            report, to: &rows, host: activeHost, instance: instance,
            selected: selectedSession, visiblePanes: [])
        applyStatusEffects(effects)
        refreshSidebar()
        // A status-only replay can precede any new screen, even when since is current
        // and the host has no delta to send. Ask for one picture before auto-ack.
        if swappedFrom == nil, let session = selectedSession,
           rows[session]?.panes.contains(report.pane) == true,
           !connection.receivedScreen.contains(report.pane),
           !requested.contains(report.pane) {
            sendAttach(report.pane, force: true, resume: false)
        }
        markVisibleStatusesSeen()
    }

    @objc func statusWindowBecameVisible(_ notification: Notification) {
        markVisibleStatusesSeen()
    }
}
