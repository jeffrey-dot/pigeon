import AppKit
import PigeonCore

// Switching machines, plus the menu actions that drive the sidebar.

extension AppDelegate {
    @objc func selectHost(_ sender: NSMenuItem) {
        guard let host = sender.representedObject as? String else { return }
        switchTo(host: host)
    }

    @objc func stopLocalHost(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = "End all sessions on this Mac?"
        alert.informativeText =
            "Every session on this Mac ends, including programs still running in them."
        alert.addButton(withTitle: "End All")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard let client = clients["local"] else {
            let failure = NSAlert()
            failure.messageText = "No sessions are running on this Mac"
            failure.runModal()
            return
        }
        // The socket drop would normally restart the host; this time it must stay down.
        stoppedByUser = true
        onHost("local") { send({ _ in }, type: PG_MSG_SHUTDOWN) }
        client.flushWrites()
    }

    @objc func useThisMac(_ sender: Any?) {
        switchTo(host: nil)
    }

    @objc func connectToHost(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = "Connect to a Server"
        alert.informativeText =
            "Enter a server you can sign in to with SSH, like user@example.com or a name "
            + "from your SSH config. Pigeon sets itself up there the first time, so your "
            + "sessions keep running after you close the window."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.placeholderString = "user@example.com"
        alert.accessoryView = field
        alert.addButton(withTitle: "Connect")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        switch HostName.parse(field.stringValue) {
        case let .success(host):
            switchTo(host: host)
        case .failure(.empty):
            return
        case let .failure(problem):
            let refusal = NSAlert()
            refusal.messageText = "Couldn't connect"
            refusal.informativeText = problem.description
            refusal.addButton(withTitle: "OK")
            refusal.runModal()
        }
    }

    /// Brings a machine to the front. Every other host keeps its connection, so switching is
    /// instant and their sessions stay live in the sidebar (`docs/remote.md` §4).
    private func switchTo(host: String?) {
        let key = hostKey(host)
        if key != "local", !hosts.hosts.contains(key) {
            hosts.hosts.append(key)
            buildMenu()
        }
        hosts.current = key == "local" ? nil : key
        hostsStore.save(hosts)
        activate(host: key)
    }

    /// Runs `body` with every per-host accessor (`rows`, `connection`, the send helpers)
    /// pointed at `host`, then points them back, however `body` returns. While a background
    /// host is in scope `swappedFrom` names the visible one, and sidebar redraws wait for it.
    func onHost(_ host: String, _ body: () -> Void) {
        let previous = activeHost
        let previousSwap = swappedFrom
        activeHost = host
        if host != previous { swappedFrom = previousSwap ?? previous }
        body()
        activeHost = previous
        swappedFrom = previousSwap
        // Drawn only once the visible host is back: the sidebar selection and the header
        // describe what the window shows, never a background machine's state.
        if swappedFrom == nil, sidebarDirty {
            sidebarDirty = false
            refreshSidebar()
        }
    }

    /// Shows `host`'s terminal, connecting it if it is not up yet.
    func activate(host: String) {
        debugLog("activate host=\(host) from=\(activeHost)")
        if host != activeHost {
            activeHost = host
            selectedSession = nil
            localSession = nil
            container.apply(.empty)
            // A degraded session pointed these at its local VT; the protocol path takes over.
            container.onPaneInput = { [weak self] pane, bytes in self?.sendKeys(pane, bytes) }
        container.onPanePaste = { [weak self] pane, text in self?.sendPaste(pane, text) }
            container.onPaneResize = { [weak self] pane, cols, rows in
                self?.sendResize(pane, cols: cols, rows: rows)
            }
        }
        if clients[host] == nil {
            if host == "local" {
                if !connectHost() {
                    hostLost(host: "local", "Pigeon's background service isn't running.")
                }
            } else {
                startRemote(host, select: true)
            }
        }
        if selectedSession == nil, let first = (connections[host]?.rows ?? [:]).keys.sorted().first {
            select(rowId(host: host, session: first))
        }
        refreshSidebar()
    }

    @objc func nextNeedsYou(_ sender: Any?) {
        let sessions = sidebarSessions()
        let current = selectedSession.map { rowId(host: activeHost, session: $0) }
        guard let next = SidebarModel.nextNeedsYou(sessions, after: current) else { return }
        select(next.id)
    }

    @objc func newSession(_ sender: Any?) {
        sendNewSession(cwd: rows[selectedSession ?? -1]?.cwd)
    }

    /// The sidebar `+` dropdown: a new session on any connected machine, or a way to reach one.
    func buildNewSessionMenu() -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let items = SidebarModel.newSessionItems(
            hosts: hosts.hosts,
            connected: Set(clients.keys),
            connecting: connectingHost,
            localName: localMachineName)
        for entry in items {
            if entry.action == .connectNew { menu.addItem(.separator()) }
            let item = NSMenuItem(
                title: entry.title, action: #selector(newSessionMenuPicked(_:)), keyEquivalent: "")
            item.target = self
            item.isEnabled = entry.enabled
            item.representedObject = NewSessionChoice(entry.action)
            // Same shortcut as the Hosts menu item; ⌘T is not shown because it follows the
            // machine on screen, which is not always this Mac.
            if entry.action == .connectNew { item.keyEquivalent = "O" }
            menu.addItem(item)
        }
        return menu
    }

    @objc private func newSessionMenuPicked(_ sender: NSMenuItem) {
        guard let choice = sender.representedObject as? NewSessionChoice else { return }
        switch choice.action {
        case let .newSession(host):
            // The new row must land in the window: `created` only selects on the visible host.
            if host != activeHost { switchTo(host: host == "local" ? nil : host) }
            sendNewSession(cwd: rows[selectedSession ?? -1]?.cwd)
        case let .connect(host):
            switchTo(host: host)
        case .connectNew:
            connectToHost(sender)
        }
    }

    /// ⌘⇧↩ zooms the focused pane; pressing it again gives the siblings their space back.
    func toggleZoom() {
        guard let type = SplitContainerView.zoomRequest(
            currentZoom: container.model.zoom, focused: focusedPane ?? 0)
        else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", focusedPane ?? 0)
        }, type: type)
    }

    @objc func closeSelectedSession(_ sender: Any?) {
        guard let selectedSession else { return }
        closeSession(selectedSession)
    }

    func sidebarSessions() -> [SidebarSession] {
        var sessions: [SidebarSession] = []
        // Sessions of every *connected* host, grouped under `host / project`. An offline host
        // simply disappears from the sidebar; its sessions come back when it reconnects.
        for key in hostKeys() where clients[key] != nil {
            let hostRows = (connections[key]?.rows ?? [:]).values.sorted { $0.id < $1.id }
            for row in hostRows {
                sessions.append(
                    SidebarSession(
                        id: rowId(host: key, session: row.id),
                        title: state.renamed[unreadKey(host: key, session: row.id)] ?? row.app,
                        group: row.group,
                        status: row.status,
                        unread: row.unread,
                        app: row.app,
                        cwd: row.cwd,
                        lastSeen: row.lastSeen,
                        reported: row.seq > 0,
                        sizeSource: row.sizeSource,
                        clients: row.clients,
                        host: key,
                        agent: row.agent
                    ))
            }
        }
        return sessions
    }

    func refreshSidebar() {
        // Inside a background host's message the active host is swapped; draw afterwards.
        if swappedFrom != nil {
            sidebarDirty = true
            return
        }
        let order = state.order.compactMap(SessionRef.init(key:)).map { sessionRefs.id(for: $0) }
        let raw = SidebarModel.sections(sidebarSessions(), order: order)
        let sections = raw.map { section in
            // The heading is the user's group name; it must not gain markers or directory names
            // as the session moves around. The machine is its own heading above it.
            var named = section
            named.group = state.renamedGroups[section.key] ?? section.group
            return named
        }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let needs = SidebarModel.needsYou(sidebarSessions())
            let perHost = hostKeys()
                .map { "\($0):\(connections[$0]?.rows.count ?? -1)" }
                .joined(separator: ",")
            debugLog(
                "sidebar active=\(activeHost) perHost=\(perHost) sessions=\(rows.count) needs=\(needs) "
                    + "selected=\(selectedSession.map(String.init) ?? "-") "
                    + "groups=\(sections.map(\.key).joined(separator: "|"))")
        }
        let selectedRow = selectedSession.map { rowId(host: activeHost, session: $0) }
        let localName = localMachineName
        sidebar.update(sections: sections, selected: selectedRow) { host in
            SidebarModel.machineLabel(host: host, localName: localName)
        }
        // Idempotent: the self-test starts on the first refresh that has a session on screen
        // (more than one path can end a session-list update early).
        beginSelfTestIfNeeded()
        let needs = SidebarModel.needsYou(sidebarSessions())
        window.title = needs > 0 ? "pigeon — \(needs) needs you" : "pigeon"
        updateHeader()
    }

    /// `docs/ui.md` §3: name, cwd, and the same status wording as the sidebar.
    private func updateHeader() {
        guard let row = selectedSession.flatMap({ rows[$0] }) else {
            header.update(title: "", cwd: "", badge: "")
            return
        }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let cwd = row.cwd.isEmpty ? "" : row.cwd.replacingOccurrences(of: home, with: "~")
        let session = sidebarSessions().first { $0.id == rowId(host: activeHost, session: row.id) }
        header.update(
            title: session.map(SidebarModel.rowTitle(for:)) ?? row.app,
            cwd: cwd,
            // No OSC 7501 record means no state to report: leave the badge blank.
            badge: session.map { SidebarModel.headerBadge(for: $0) } ?? "",
            detail: session.flatMap { SidebarModel.detailLine(for: $0) }
        )
    }

    /// light → dark → follow the system. Clearing `NSApp.appearance` hands control back to
    /// the system, which is the only way to undo a pin without relaunching.
    @objc func toggleAppearance(_ sender: Any?) {
        let mode = AppearanceMode.current().next
        NSApp.appearance = mode.appearance
        debugLog("appearance -> \(mode.rawValue)")
        // No explicit send here: the `effectiveAppearance` observer fires on this change too,
        // and sending twice would just duplicate the message.
    }

    @objc func toggleOptionAsMeta(_ sender: NSMenuItem) {
        settings.optionAsMeta.toggle()
        settingsStore.save(settings)
        container.optionAsMeta = settings.optionAsMeta
        sender.state = settings.optionAsMeta ? .on : .off
    }

    @objc func toggleSidebar(_ sender: Any?) {
        sidebar.view.isHidden.toggle()
        // A hidden arranged subview comes back at width 0, which looks like "cannot reopen".
        if !sidebar.view.isHidden {
            splitView.setPosition(SidebarMetrics.width, ofDividerAt: 0)
        }
        // With no sidebar the traffic lights sit over the right pane, so its header
        // pads past them (harp's `CARD_HEADER_PAD_CLOSED`).
        header.setSidebarCollapsed(sidebar.view.isHidden)
    }
}
