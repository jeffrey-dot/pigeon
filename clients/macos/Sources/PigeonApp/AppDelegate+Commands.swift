import AppKit
import PigeonCore

// Outbound commands: host and session id mapping, and every `send*` the window issues.

extension AppDelegate {
    /// `"local"` plus every known remote, in a stable order.
    func hostKeys() -> [String] {
        hostOrder = ["local"] + hosts.hosts.filter { $0 != "local" }
        return hostOrder
    }

    func hostKey(_ host: String?) -> String { host ?? "local" }

    /// The sidebar's Int id for a session (`SessionRefs`): stable for the run whatever the
    /// host list does, so adding or forgetting a machine never renames another's rows.
    func rowId(host: String, session: Int) -> Int {
        sessionRefs.id(for: SessionRef(host: host, session: session))
    }

    func splitRowId(_ id: Int) -> (host: String, session: Int)? {
        sessionRefs.ref(for: id).map { ($0.host, $0.session) }
    }

    /// Legacy keys also identify user-assigned session names.
    func unreadKey(host: String, session: Int) -> String {
        SessionRef(host: host, session: session).key
    }

    func requestID() -> UInt32 {
        defer { nextRequest &+= 1 }
        return nextRequest
    }

    func send(_ build: (OpaquePointer) -> Void, type: PgMsgType) {
        guard let client, let message = Wire.make(type) else { return }
        sentCounts[type, default: 0] += 1
        defer { Wire.free(message) }
        build(message)
        client.send(message)
    }

    /// Reports the palette the panes should answer colour queries with. Sent after the
    /// handshake and again whenever the appearance changes.
    func sendAppearance() {
        let appearance = NSApp.effectiveAppearance
        let ansi = TerminalPalette.hexValues(appearance: appearance)
        guard ansi.count == 16 else { return }
        let fg = TerminalPalette.hex(SidebarTheme.terminalForeground, appearance: appearance)
        let bg = TerminalPalette.hex(SidebarTheme.terminalBackground, appearance: appearance)
        send({ message in
            Wire.set(message, u32: "fg", fg)
            Wire.set(message, u32: "bg", bg)
            Wire.set(message, u32Array: "ansi", ansi)
        }, type: PG_MSG_APPEARANCE)
        debugLog("appearance sent fg=\(String(format: "%06x", fg)) bg=\(String(format: "%06x", bg))")
    }

    func sendHello() {
        send({ message in
            Wire.set(message, u32: "proto", UInt32(pg_proto_version()))
            Wire.set(message, str: "client", "pigeon-mac")
            // `delta` sends only the rows that changed: a keystroke costs ~2 KB instead of a
            // full screen (~100 KB on a large pane), which is what makes ssh feel slow.
            Wire.set(message, strings: "features", ["scrollback", "delta", "status_all", "status_snapshot", "text_query", "text_viewport"])
        }, type: PG_MSG_HELLO)
    }

    func debugLog(_ text: String) {
        guard ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil else { return }
        FileHandle.standardError.write(Data("pigeon: \(text)\n".utf8))
    }

    func requestSessions() {
        send({ _ in }, type: PG_MSG_SESSIONS)
    }

    func sendNewSession(cwd: String?) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            if let cwd { Wire.set(message, str: "cwd", cwd) }
        }, type: PG_MSG_NEW_SESSION)
    }

    /// Attaching is also how the host learns which session this window follows: it switches
    /// `state.session` to that session and replies with its layout + snapshot. The startup pass
    /// attaches every session (for the sidebar's status), so selecting a session has to force
    /// a re-attach — otherwise the host keeps following whichever session it attached last and
    /// the window never receives the selected session's layout.
    func sendAttach(_ pane: UInt32, force: Bool = false, resume: Bool = true) {
        guard pane != 0 else { return }
        let stale = connection.stale?.contains(pane) ?? false
        if !force, (haveSnapshot.contains(pane) && !stale) || !requested.insert(pane).inserted {
            return
        }
        connection.stale?.remove(pane)
        let since = resume ? HostRecovery.resumeSeq(
            drawn: frameSeq[pane], stale: stale, onScreen: haveSnapshot.contains(pane)) : nil
        // A caught-up since request can legitimately return no screen. Only a full
        // attach promises the picture that status confirmation waits for.
        if since == nil { requested.insert(pane) } else { requested.remove(pane) }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", pane)
            if let since { Wire.set(message, u32: "since", since) }
            // Scrollback is read through `scroll`, so the history (up to 2 MB) never needs to
            // ride along — every session click re-attaches.
            Wire.set(message, bool: "history", false)
        }, type: PG_MSG_ATTACH)
    }

    /// The sidebar shows every session's agent. A host with `status_all` already sends those;
    /// an older one only reports panes this window attached, so each session's active pane
    /// is attached (which also counts this window in that session's size vote).
    func watchStatuses() {
        guard !connection.statusAll else { return }
        for row in rows.values { sendAttach(row.activePane) }
    }

    /// `scroll` takes a delta: negative looks back, `0` is the live screen (protocol §3.3).
    func sendScroll(_ pane: UInt32, lines: Int) {
        if searchModel?.context.pane == pane, searchModel?.context.host == activeHost { closeSearch(focus: false) }
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, i32: "offset", -Int32(lines))
        }, type: PG_MSG_SCROLL)
    }

    func sendPaste(_ pane: UInt32, _ text: String) {
        if searchModel?.context.pane == pane, searchModel?.context.host == activeHost { closeSearch(focus: false) }
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, str: "text", text)
        }, type: PG_MSG_PASTE)
    }

    func sendKeys(_ pane: UInt32, _ bytes: [UInt8]) {
        if searchModel?.context.pane == pane, searchModel?.context.host == activeHost { closeSearch(focus: false) }
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, bytes: "bytes", bytes)
        }, type: PG_MSG_KEYS)
    }

    func sendResize(_ pane: UInt32, cols: UInt16, rows: UInt16) {
        if searchModel?.context.pane == pane, searchModel?.context.host == activeHost {
            searchModel?.expire(); clearSearchHighlight(); refreshSearch()
        }
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, u32: "cols", UInt32(cols))
            Wire.set(message, u32: "rows", UInt32(rows))
        }, type: PG_MSG_RESIZE_PANE)
    }

    func sendClosePane(_ pane: UInt32) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", pane)
        }, type: PG_MSG_CLOSE_PANE)
    }

    func split(_ axis: PgAxis) {
        guard let focusedPane else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", focusedPane)
            Wire.set(message, str: "axis", axis == PG_AXIS_VERTICAL ? "v" : "h")
            Wire.set(message, f32: "ratio", 0.5)
        }, type: PG_MSG_SPLIT_PANE)
    }

    func closeFocusedPane() {
        guard let focusedPane else { return }
        sendClosePane(focusedPane)
    }

    func focus(_ pane: UInt32) {
        if searchModel?.context.pane != pane { closeSearch(focus: false) }
        focusedPane = pane
        container.focusPane(pane)
        send({ message in Wire.set(message, u32: "pane_id", pane) }, type: PG_MSG_FOCUS)
        markVisibleStatusesSeen()
    }

    func resizeSplit(_ split: UInt32, _ ratio: Double) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "split_id", split)
            Wire.set(message, f32: "ratio", Float(ratio))
        }, type: PG_MSG_RESIZE_SPLIT)
    }
}
