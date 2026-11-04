import AppKit
import PigeonCore

extension AppDelegate {
    @objc func showStatusRecords(_ sender: Any?) {
        guard let session = selectedSession else { return }
        showStatusRecords(for: rowId(host: activeHost, session: session))
    }

    func showStatusRecords(for id: Int) {
        guard let ref = sessionRefs.ref(for: id),
              let host = connections[ref.host], host.ready,
              let instance = host.instance, host.rows[ref.session] != nil else { return }
        if activeHost != ref.host || selectedSession != ref.session { select(id) }
        closeStatusPanel()
        let panel = StatusRecordsPanel(inspection: StatusInspection(
            session: ref, instance: instance, generation: host.generation))
        panel.onSelect = { [weak self] target in self?.selectStatusRecord(target) }
        panel.onAcknowledge = { [weak self] target in self?.acknowledgeStatusRecord(target) }
        statusPanel = panel
        refreshStatusPanel()
        panel.center()
        window.addChildWindow(panel, ordered: .above)
        panel.orderFront(nil)
    }

    func closeStatusPanel() {
        guard let panel = statusPanel else { return }
        window?.removeChildWindow(panel)
        panel.close()
        statusPanel = nil
    }

    func refreshStatusPanel() {
        guard swappedFrom == nil, let panel = statusPanel else { return }
        guard connection.ready,
              panel.inspection.matches(host: activeHost, session: selectedSession,
                                       instance: connection.instance, generation: connection.generation),
              let row = rows[panel.inspection.session.session] else {
            closeStatusPanel()
            return
        }
        panel.update(records: SessionReducer.pendingRecords(in: row), visiblePanes: visibleStatusPanes)
    }

    private func selectStatusRecord(_ target: StatusAcknowledgement) {
        guard let panel = statusPanel, panel.inspection.selected?.target == target,
              panel.inspection.matches(host: activeHost, session: selectedSession,
                                       instance: connection.instance, generation: connection.generation),
              connection.accepts(target, host: activeHost) else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", target.pane)
        }, type: PG_MSG_UNZOOM)
        focus(target.pane)
        if !connection.receivedScreen.contains(target.pane) {
            sendAttach(target.pane, force: true, resume: false)
        }
        refreshStatusPanel()
    }

    private func acknowledgeStatusRecord(_ target: StatusAcknowledgement) {
        guard let panel = statusPanel, panel.isVisible,
              panel.occlusionState.contains(.visible), panel.inspection.selected?.target == target,
              panel.inspection.matches(host: activeHost, session: selectedSession,
                                       instance: connection.instance, generation: connection.generation),
              let session = selectedSession, connection.accepts(target, host: activeHost) else { return }
        let effects = SessionReducer.markSeen(target, in: session, rows: &rows, visiblePanes: visibleStatusPanes)
        applyStatusEffects(effects)
        refreshSidebar()
    }
}
