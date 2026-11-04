import AppKit
import PigeonCore

// Inbound protocol messages: decode, apply, and the self test.

extension AppDelegate {
    /// Routes one host's message through the handlers, which read and write "the active
    /// host" state; the window itself only follows the visible host.
    func handle(_ message: OpaquePointer, host: String) {
        connections[host]?.heartbeat?.heard(at: Date())
        let visible = host == activeHost
        onHost(host) { handleActive(message, visible: visible) }
    }

    private func handleActive(_ message: OpaquePointer, visible: Bool) {
        switch Wire.type(message) {
        case PG_MSG_HELLO_ACK:
            let instance = Wire.string(message, "instance")
            if connection.instance != nil, connection.instance != instance {
                for row in rows.values { state.unread[SessionRef(host: activeHost, session: row.id).key] = nil }
                store.save(state)
            }
            connection.instanceChanged(to: instance)
            connection.statusAll = Wire.contains(message, strings: "features", "status_all")
            // Tell the host what we are drawing with, so programs inside the panes can pick
            // their light or dark theme (`OSC 4/10/11`).
            sendAppearance()
        case PG_MSG_SESSIONS:
            applySessions(message, visible: visible)
        case PG_MSG_CREATED:
            // Jump to the new session as soon as the refreshed list includes it.
            pendingSelectSession = Int(Wire.u32(message, "session_id"))
            requestSessions()
        case PG_MSG_LAYOUT:
            guard let model = LayoutModel(message: message) else { break }
            let sessionID = Int(Wire.u32(message, "session_id"))
            debugLog("layout session=\(sessionID) selected=\(selectedSession.map(String.init) ?? "-") panes=\(model.panes.map(\.id))")
            if var row = rows[sessionID] {
                if model.focus != 0 { row.activePane = model.focus }
                rows[sessionID] = row
            }
            guard sessionID == selectedSession else { break }
            if model.focus != 0 { focusedPane = model.focus }
            guard visible else { break }
            container.apply(model)
            displayedHost = activeHost
            displayedSession = sessionID
            flushPendingSnapshots()
            for pane in model.panes.map(\.id) { sendAttach(pane) }
            markVisibleStatusesSeen()
        case PG_MSG_SNAPSHOT:
            guard let snapshot = pg_msg_snapshot(message) else { break }
            defer { pg_snapshot_free(snapshot) }
            let pane = snapshot.pointee.pane_id
            focusedPane = focusedPane ?? pane
            // Background hosts keep their rows and status; only the visible one draws.
            guard visible else { break }
            if container.applySnapshot(snapshot) {
                haveSnapshot.insert(pane)
                frameSeq[pane] = Wire.u32(message, "seq")
                // `scroll` answers carry where the view is; plain snapshots do not, and then
                // the indicator stays hidden (both fields are 0).
                container.paneView(pane)?.setScrollPosition(
                    total: Int(Wire.u32(message, "history_len")),
                    offset: Int(Wire.u32(message, "offset")))
                container.paneView(pane)?.modes = Wire.u32(message, "modes")
                markVisibleStatusesSeen()
            } else if let frame = Wire.frame(message) {
                pendingSnapshots[pane] = frame
            }
        case PG_MSG_DELTA:
            applyDelta(message, visible: visible)
        case PG_MSG_STATUS:
            applyStatus(message)
        case PG_MSG_SIZE_STATE:
            // Protocol §6: more than one window means the user should be able to tell who
            // decided the size, so it rides along in the row's age label.
            let pane = Wire.u32(message, "pane_id")
            guard let id = rows.first(where: { $0.value.panes.contains(pane) })?.key,
                  var row = rows[id]
            else { break }
            row.sizeSource = Wire.string(message, "source")
            row.clients = Int(Wire.u32(message, "clients"))
            rows[id] = row
            refreshSidebar()
        case PG_MSG_PANE_EXITED:
            // The shell is gone (`exit`, Ctrl-D, a crash). The host keeps an exited pane until
            // `close_pane`; closing just that pane lets its split siblings keep running, and
            // the host drops the session itself once its last pane is gone.
            let pane = Wire.u32(message, "pane_id")
            guard let id = rows.first(where: { $0.value.panes.contains(pane) })?.key,
                  let row = rows[id]
            else { break }
            if row.panes.count > 1 {
                debugLog("pane exited pane=\(pane) -> closing that pane of session \(id)")
                sendClosePane(pane)
                break
            }
            debugLog("pane exited pane=\(pane) -> closing session \(id)")
            closeSession(host: activeHost, session: id)
        case PG_MSG_HELLO_ERROR:
            // Retrying a refused handshake only repeats it, so stop and explain.
            let host = activeHost
            debugLog("hello refused: \(Wire.string(message, "reason")) \(Wire.string(message, "detail"))")
            refusedReason = HostRecovery.refusal(
                reason: Wire.string(message, "reason"),
                machine: host == "local" ? "This Mac" : host)
            // `close()` is the quiet hang-up (no `onClose`), so report the refusal here.
            client?.close()
            DispatchQueue.main.async { [weak self] in
                self?.hostLost(host: host, "The connection was refused.")
            }
        case PG_MSG_ERROR:
            NSLog("pigeon: \(Wire.string(message, "code")): \(Wire.string(message, "detail"))")
        default:
            break
        }
    }

    /// Patches a pane with the rows that changed. Anything that does not line up — a missed
    /// frame, a grid of another size, a pane still waiting for its snapshot — falls back to
    /// a fresh `attach`, the protocol's only recovery path.
    private func applyDelta(_ message: OpaquePointer, visible: Bool) {
        guard visible, let delta = pg_msg_delta(message) else { return }
        defer { pg_delta_free(delta) }
        let pane = delta.pointee.pane_id
        guard haveSnapshot.contains(pane), let view = container.paneView(pane) else { return }
        let drawn = frameSeq[pane] ?? 0
        // Queued before our snapshot was taken: the snapshot already shows these rows.
        if delta.pointee.seq <= drawn { return }
        let expected = drawn + 1
        if delta.pointee.seq == expected, view.apply(delta: delta) {
            frameSeq[pane] = delta.pointee.seq
            // Every delta carries the modes; a host too old to send them leaves them alone.
            let unreported = UInt32.max
            let modes = Wire.u32(message, "modes", fallback: unreported)
            if modes != unreported { view.modes = modes }
            return
        }
        debugLog("delta pane=\(pane) seq=\(delta.pointee.seq) expected=\(expected): re-attach")
        haveSnapshot.remove(pane)
        frameSeq[pane] = nil
        sendAttach(pane, force: true)
    }

    private func applySessions(_ message: OpaquePointer, visible: Bool) {
        let count = pg_msg_sessions_count(message)
        debugLog("sessions from active=\(activeHost) count=\(count)")
        var listed: [SessionReducer.Listed] = []
        for index in 0..<count {
            var info = PgSessionInfo()
            guard pg_msg_session_at(message, index, &info), info.pane_count > 0 else { continue }
            var panes = [UInt32](repeating: 0, count: Int(info.pane_count))
            let written = panes.withUnsafeMutableBufferPointer { buffer in
                pg_msg_session_panes(message, index, buffer.baseAddress, buffer.count)
            }
            guard written > 0 else { continue }
            let program = Wire.string(info.active_program)
            listed.append(SessionReducer.Listed(
                id: Int(info.session_id),
                panes: Array(panes.prefix(written)),
                cwd: Wire.string(info.cwd),
                activePane: info.active_pane,
                program: program.isEmpty ? Wire.string(info.active_title) : program))
        }
        let host = activeHost
        let saved = state.unread
        let updated = SessionReducer.merge(
            listed, into: rows, host: host, now: Date(),
            unread: { saved[$0.key] ?? 0 },
            gitRoot: { GitRoot.find(cwd: $0, onThisMac: host == "local") })
        rows = updated
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let apps = rows.values.map { "\($0.id):\($0.app)" }.sorted().joined(separator: " ")
            FileHandle.standardError.write(Data("pigeon: sessions apps=\(apps)\n".utf8))
        }
        let firstList = listedHosts.insert(activeHost).inserted
        if SidebarModel.shouldStartFirstSession(listIsEmpty: rows.isEmpty, firstList: firstList) {
            // An empty host gets one session instead of an empty group.
            debugLog("no sessions on \(activeHost): creating one")
            sendNewSession(cwd: nil)
            return
        }
        // A background host keeps its sessions and status; it must not steal the window.
        guard visible else {
            watchStatuses()
            refreshSidebar()
            return
        }
        if let pending = pendingSelectSession, rows[pending] != nil {
            pendingSelectSession = nil
            select(rowId(host: activeHost, session: pending))
            return
        }
        if selectedSession == nil || rows[selectedSession!] == nil {
            selectedSession = rows.keys.sorted().first
        }
        watchStatuses()
        if let selectedSession, let row = rows[selectedSession] {
            focusedPane = row.activePane
            // Force the selected session's panes: `attach` is what makes the host follow this
            // session and reply with its layout + snapshot. Without it a fresh launch shows an
            // empty pane until the first click (which selects the already-selected row).
            for pane in row.panes { sendAttach(pane, force: true) }
            send({ message in Wire.set(message, u32: "pane_id", row.activePane) }, type: PG_MSG_FOCUS)
        }
        refreshSidebar()
        markVisibleStatusesSeen()
    }

    private func applyStatus(_ message: OpaquePointer) {
        let report = SessionReducer.Report(
            pane: Wire.u32(message, "pane_id"),
            status: SidebarModel.status(from: Wire.string(message, "state")),
            seq: Wire.u32(message, "seq"),
            agent: AgentInfo(message: message))
        // Only the session on screen counts as seen: a background host's own selection is
        // not something the user is looking at.
        let effects = SessionReducer.apply(
            report, to: &rows, host: activeHost, selected: selectedSession,
            onScreen: swappedFrom == nil, visiblePanes: visibleStatusPanes)
        applyStatusEffects(effects)
        refreshSidebar()
    }

    private func flushPendingSnapshots() {
        for (pane, frame) in pendingSnapshots {
            var consumed = 0
            var decoded: OpaquePointer?
            frame.withUnsafeBytes { raw in
                decoded = pg_msg_decode_frame(
                    raw.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    raw.count,
                    &consumed
                )
            }
            guard let decoded else { continue }
            defer { Wire.free(decoded) }
            guard let snapshot = pg_msg_snapshot(decoded) else { continue }
            defer { pg_snapshot_free(snapshot) }
            if container.applySnapshot(snapshot) {
                haveSnapshot.insert(pane)
                frameSeq[pane] = Wire.u32(decoded, "seq")
                // `scroll` answers carry where the view is; plain snapshots do not, and then
                // the indicator stays hidden (both fields are 0).
                container.paneView(pane)?.setScrollPosition(
                    total: Int(Wire.u32(decoded, "history_len")),
                    offset: Int(Wire.u32(decoded, "offset")))
                container.paneView(pane)?.modes = Wire.u32(decoded, "modes")
                pendingSnapshots.removeValue(forKey: pane)
            }
        }
    }
}
