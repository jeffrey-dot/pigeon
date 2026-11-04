import AppKit
import PigeonCore

// Session list actions: select, close, rename, reorder, unread.

extension AppDelegate {
    func select(_ id: Int) {
        lastSelectedId = id
        guard let (host, session) = splitRowId(id) else { return }
        if host != activeHost {
            activate(host: host)
        }
        activeHost = host
        guard let row = connections[host]?.rows[session] else { return }
        debugLog("select host=\(host) id=\(session) activePane=\(row.activePane) panes=\(row.panes)")
        selectedSession = session
        focusedPane = row.activePane
        // Clicking a session must hand the keyboard back to its terminal: the click left
        // focus on the sidebar table, and nothing else would move it back when the layout
        // does not change.
        container.focusPane(row.activePane)
        for pane in row.panes { sendAttach(pane, force: true) }
        send({ message in Wire.set(message, u32: "pane_id", row.activePane) }, type: PG_MSG_FOCUS)
        // The layout/snapshot response marks only the panes actually shown as seen.
        refreshSidebar()
    }

    func closeSession(_ id: Int) {
        guard let (host, session) = splitRowId(id) else { return }
        closeSession(host: host, session: session)
    }

    /// Closing a session on a specific host (the pane-exited path knows which one).
    func closeSession(host: String, session: Int) {
        onHost(host) { closeSessionOnActiveHost(session) }
    }

    private func closeSessionOnActiveHost(_ session: Int) {
        let host = activeHost
        let id = session
        guard let row = rows[id] else { return }
        for pane in row.panes { sendClosePane(pane) }
        rows.removeValue(forKey: id)
        state.unread.removeValue(forKey: unreadKey(host: host, session: session))
        store.save(state)
        if selectedSession == id { selectedSession = nil }
        refreshSidebar()
        requestSessions()
        showAnotherMachineIfEmpty()
        quitIfNoSessionsLeft()
    }

    /// The machine on screen just lost its last session (its shell exited, or it was closed):
    /// move the window to another machine's session instead of leaving the closed session's
    /// last screen up. A background machine emptying changes nothing on screen.
    private func showAnotherMachineIfEmpty() {
        guard swappedFrom == nil, rows.isEmpty else { return }
        let order = state.order.compactMap(SessionRef.init(key:)).map { sessionRefs.id(for: $0) }
        let sections = SidebarModel.sections(sidebarSessions(), order: order)
        guard let next = SidebarModel.sessionToShow(afterEmptying: activeHost, sections: sections) else {
            container.apply(.empty)
            return
        }
        debugLog("no sessions left on \(activeHost): showing \(next)")
        // `select` moves `activeHost`; the close runs inside `onHost`, which would put it back.
        DispatchQueue.main.async { [weak self] in self?.select(next) }
    }

    /// Closing the last session (from the sidebar, ⌘⇧W, or its shell exiting) quits the app,
    /// like closing a terminal's last tab. Only a close leads here: a host that drops its
    /// connection, or one that has no sessions yet at launch, never quits pigeon.
    private func quitIfNoSessionsLeft() {
        let remaining = hostKeys().filter { clients[$0] != nil }
            .reduce(0) { $0 + (connections[$1]?.rows.count ?? 0) }
        guard SidebarModel.shouldQuitAfterClose(
            remainingSessions: remaining, creatingSession: pendingSelectSession != nil)
        else { return }
        debugLog("last session closed: quitting")
        // `applicationWillTerminate` waits for the queued `close_pane` frames to be written.
        DispatchQueue.main.async { NSApp.terminate(nil) }
    }

    func markRead(_ id: Int) {
        guard let (host, session) = splitRowId(id) else { return }
        onHost(host) { markReadOnActiveHost(session) }
    }

    private func markReadOnActiveHost(_ session: Int) {
        let host = activeHost
        guard let row = rows[session] else { return }
        let effects = SessionReducer.markSeen(Set(row.panes), in: session, rows: &rows, host: host)
        applyStatusEffects(effects)
        refreshSidebar()
    }

    func rename(_ id: Int, _ name: String) {
        guard let (host, session) = splitRowId(id) else { return }
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let key = unreadKey(host: host, session: session)
        // An empty name gives the row its program name back.
        state.renamed[key] = trimmed.isEmpty ? nil : trimmed
        store.save(state)
        refreshSidebar()
    }

    func renameGroup(_ group: String, _ name: String) {
        state.renamedGroups[group] = name
        store.save(state)
        refreshSidebar()
    }

    func reorder(_ order: [Int]) {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: sidebar reorder \(order)\n".utf8))
        }
        // Persisted as `SessionRef` keys: the run's Int ids mean nothing next launch.
        state.order = order.compactMap { sessionRefs.ref(for: $0)?.key }
        store.save(state)
        refreshSidebar()
    }
}
