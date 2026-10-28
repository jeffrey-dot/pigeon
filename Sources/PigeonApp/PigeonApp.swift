import AppKit
import PigeonCore

/// Wires the window to a `pigeon-host`: sidebar on the left, split container on
/// the right, and the protocol messages that keep both in sync.
final class AppDelegate: NSObject, NSApplicationDelegate {
    private struct Row {
        var id: Int
        var panes: [UInt32]
        var cwd: String
        var activePane: UInt32
        var app: String
        var title: String
        var status: SidebarSession.Status
        var seq: UInt32
        var unread: Int
        var lastSeen: Date

        var group: String {
            guard !cwd.isEmpty else { return "~" }
            return Grouping.groupName(forCwd: cwd, gitRoot: GitRoot.find(cwd: cwd))
        }
    }

    private var window: NSWindow!
    private var splitView: NSSplitView!
    private var sidebar: SidebarViewController!
    private var container: SplitContainerView!
    private var client: HostClient?

    private var rows: [Int: Row] = [:]
    private var selectedSession: Int?
    private var focusedPane: UInt32?
    private var requested: Set<UInt32> = []
    private var haveSnapshot: Set<UInt32> = []
    private var pendingSnapshots: [UInt32: Data] = [:]
    private var nextRequest: UInt32 = 1

    private let store = SidebarStore()
    private var state = SidebarState()

    func applicationDidFinishLaunching(_ notification: Notification) {
        state = store.load()
        buildMenu()
        buildWindow()
        connect()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    // MARK: - window

    private func buildWindow() {
        let frame = NSRect(x: 0, y: 0, width: 1100, height: 600)
        window = NSWindow(
            contentRect: frame,
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "pigeon"

        sidebar = SidebarViewController()
        sidebar.onSelect = { [weak self] id in self?.select(id) }
        sidebar.onClose = { [weak self] id in self?.closeSession(id) }
        sidebar.onMarkRead = { [weak self] id in self?.markRead(id) }
        sidebar.onRename = { [weak self] id, name in self?.rename(id, name) }
        sidebar.onRenameGroup = { [weak self] group, name in self?.renameGroup(group, name) }
        sidebar.onReorder = { [weak self] order in self?.reorder(order) }

        container = SplitContainerView(frame: frame)
        container.onSplit = { [weak self] axis in self?.split(axis) }
        container.onClose = { [weak self] in self?.closeFocusedPane() }
        container.onFocus = { [weak self] pane in self?.focus(pane) }
        container.onResizeSplit = { [weak self] split, ratio in self?.resizeSplit(split, ratio) }
        container.onPaneInput = { [weak self] pane, bytes in self?.sendKeys(pane, bytes) }
        container.onPaneResize = { [weak self] pane, cols, rows in
            self?.sendResize(pane, cols: cols, rows: rows)
        }

        splitView = NSSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin
        splitView.addArrangedSubview(sidebar.view)
        splitView.addArrangedSubview(container)
        sidebar.view.widthAnchor.constraint(greaterThanOrEqualToConstant: 200).isActive = true
        sidebar.view.widthAnchor.constraint(lessThanOrEqualToConstant: 340).isActive = true

        window.contentView = splitView
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(container)
        NSApp.activate(ignoringOtherApps: true)
        refreshSidebar()
    }

    private func buildMenu() {
        let mainMenu = NSMenu()
        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit pigeon", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        let viewItem = NSMenuItem()
        mainMenu.addItem(viewItem)
        let viewMenu = NSMenu(title: "View")
        viewMenu.addItem(withTitle: "Split Left/Right", action: #selector(SplitContainerView.splitVertical(_:)), keyEquivalent: "d")
        viewMenu.addItem(withTitle: "Split Top/Bottom", action: #selector(SplitContainerView.splitHorizontal(_:)), keyEquivalent: "D")
        viewMenu.addItem(withTitle: "Close Pane", action: #selector(SplitContainerView.closeFocused(_:)), keyEquivalent: "w")
        viewMenu.addItem(withTitle: "Previous Pane", action: #selector(SplitContainerView.focusPrevious(_:)), keyEquivalent: "[")
        viewMenu.addItem(withTitle: "Next Pane", action: #selector(SplitContainerView.focusNext(_:)), keyEquivalent: "]")
        viewItem.submenu = viewMenu

        let sessionItem = NSMenuItem()
        mainMenu.addItem(sessionItem)
        let sessionMenu = NSMenu(title: "Session")
        sessionMenu.addItem(withTitle: "Next Needs You", action: #selector(nextNeedsYou(_:)), keyEquivalent: "j")
        sessionMenu.addItem(withTitle: "New Session", action: #selector(newSession(_:)), keyEquivalent: "t")
        let close = NSMenuItem(title: "Close Session", action: #selector(closeSelectedSession(_:)), keyEquivalent: "W")
        sessionMenu.addItem(close)
        for item in sessionMenu.items { item.target = self }
        sessionItem.submenu = sessionMenu
        NSApp.mainMenu = mainMenu
    }

    // MARK: - host

    private func connect() {
        let path = Self.defaultSocketPath()
        do {
            let client = try HostClient.connect(path: path)
            client.onMessage = { [weak self] message in
                DispatchQueue.main.async {
                    defer { Wire.free(message) }
                    self?.handle(message)
                }
            }
            client.onClose = { [weak self] in DispatchQueue.main.async { self?.client = nil } }
            self.client = client
            sendHello()
        } catch {
            NSLog("pigeon: cannot reach \(path): \(error)")
        }
    }

    private static func defaultSocketPath() -> String {
        if let runtime = ProcessInfo.processInfo.environment["XDG_RUNTIME_DIR"] {
            return runtime + "/pigeon.sock"
        }
        return "/tmp/pigeon-\(getuid()).sock"
    }

    private func requestID() -> UInt32 {
        defer { nextRequest &+= 1 }
        return nextRequest
    }

    private func send(_ build: (OpaquePointer) -> Void, type: PgMsgType) {
        guard let client, let message = Wire.make(type) else { return }
        defer { Wire.free(message) }
        build(message)
        client.send(message)
    }

    private func sendHello() {
        send({ message in
            Wire.set(message, u32: "proto", 0x0100)
            Wire.set(message, str: "client", "pigeon-mac")
            Wire.set(message, strings: "features", ["scrollback"])
        }, type: PG_MSG_HELLO)
    }

    private func requestSessions() {
        send({ _ in }, type: PG_MSG_SESSIONS)
    }

    private func sendNewSession(cwd: String?) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            if let cwd { Wire.set(message, str: "cwd", cwd) }
        }, type: PG_MSG_NEW_SESSION)
    }

    private func sendAttach(_ pane: UInt32) {
        guard !haveSnapshot.contains(pane), requested.insert(pane).inserted else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", pane)
        }, type: PG_MSG_ATTACH)
    }

    private func sendKeys(_ pane: UInt32, _ bytes: [UInt8]) {
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, bytes: "bytes", bytes)
        }, type: PG_MSG_KEYS)
    }

    private func sendResize(_ pane: UInt32, cols: UInt16, rows: UInt16) {
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, u32: "cols", UInt32(cols))
            Wire.set(message, u32: "rows", UInt32(rows))
        }, type: PG_MSG_RESIZE_PANE)
    }

    private func sendClosePane(_ pane: UInt32) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", pane)
        }, type: PG_MSG_CLOSE_PANE)
    }

    private func sendAck(_ pane: UInt32, _ seq: UInt32) {
        guard seq > 0 else { return }
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, u32: "seq", seq)
        }, type: PG_MSG_ACK_STATUS)
    }

    private func split(_ axis: PgAxis) {
        guard let focusedPane else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", focusedPane)
            Wire.set(message, str: "axis", axis == PG_AXIS_VERTICAL ? "v" : "h")
            Wire.set(message, f32: "ratio", 0.5)
        }, type: PG_MSG_SPLIT_PANE)
    }

    private func closeFocusedPane() {
        guard let focusedPane else { return }
        sendClosePane(focusedPane)
    }

    private func focus(_ pane: UInt32) {
        focusedPane = pane
        container.focusPane(pane)
        send({ message in Wire.set(message, u32: "pane_id", pane) }, type: PG_MSG_FOCUS)
    }

    private func resizeSplit(_ split: UInt32, _ ratio: Double) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "split_id", split)
            Wire.set(message, f32: "ratio", Float(ratio))
        }, type: PG_MSG_RESIZE_SPLIT)
    }

    // MARK: - sessions

    private func select(_ id: Int) {
        guard let row = rows[id] else { return }
        selectedSession = id
        focusedPane = row.activePane
        for pane in row.panes { sendAttach(pane) }
        send({ message in Wire.set(message, u32: "pane_id", row.activePane) }, type: PG_MSG_FOCUS)
        if var current = rows[id] {
            current.unread = 0
            rows[id] = current
            state.unread[String(id)] = 0
            store.save(state)
            sendAck(current.activePane, current.seq)
        }
        refreshSidebar()
    }

    private func closeSession(_ id: Int) {
        guard let row = rows[id] else { return }
        for pane in row.panes { sendClosePane(pane) }
        rows.removeValue(forKey: id)
        state.unread.removeValue(forKey: String(id))
        store.save(state)
        if selectedSession == id { selectedSession = nil }
        refreshSidebar()
        requestSessions()
    }

    private func markRead(_ id: Int) {
        guard var row = rows[id] else { return }
        row.unread = 0
        rows[id] = row
        state.unread[String(id)] = 0
        store.save(state)
        sendAck(row.activePane, row.seq)
        refreshSidebar()
    }

    private func rename(_ id: Int, _ name: String) {
        state.renamed[String(id)] = name
        store.save(state)
        refreshSidebar()
    }

    private func renameGroup(_ group: String, _ name: String) {
        state.renamedGroups[group] = name
        store.save(state)
        refreshSidebar()
    }

    private func reorder(_ order: [Int]) {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: sidebar reorder \(order)\n".utf8))
        }
        state.order = order
        store.save(state)
        refreshSidebar()
    }

    @objc private func nextNeedsYou(_ sender: Any?) {
        let sessions = sidebarSessions()
        guard let next = SidebarModel.nextNeedsYou(sessions, after: selectedSession) else { return }
        select(next.id)
    }

    @objc private func newSession(_ sender: Any?) {
        sendNewSession(cwd: rows[selectedSession ?? -1]?.cwd)
    }

    @objc private func closeSelectedSession(_ sender: Any?) {
        guard let selectedSession else { return }
        closeSession(selectedSession)
    }

    private func sidebarSessions() -> [SidebarSession] {
        rows.values.map { row in
            SidebarSession(
                id: row.id,
                title: row.app,
                group: row.group,
                status: row.status,
                unread: row.unread,
                app: row.app,
                cwd: row.cwd,
                lastSeen: row.lastSeen
            )
        }
    }

    private func refreshSidebar() {
        let raw = SidebarModel.sections(sidebarSessions(), order: state.order)
        let sections = raw.map { section in
            (group: state.renamedGroups[section.group] ?? section.group, sessions: section.sessions)
        }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let needs = SidebarModel.needsYou(sidebarSessions())
            FileHandle.standardError.write(Data(
                "pigeon: sidebar sessions=\(rows.count) needs=\(needs) selected=\(selectedSession.map(String.init) ?? "-")\n".utf8
            ))
        }
        sidebar.update(sections: sections, selected: selectedSession)
        let needs = SidebarModel.needsYou(sidebarSessions())
        window.title = needs > 0 ? "pigeon — \(needs) needs you" : "pigeon"
    }

    // MARK: - protocol

    private func handle(_ message: OpaquePointer) {
        switch Wire.type(message) {
        case PG_MSG_HELLO_ACK:
            break
        case PG_MSG_SESSIONS:
            applySessions(message)
        case PG_MSG_CREATED:
            requestSessions()
        case PG_MSG_LAYOUT:
            guard let model = LayoutModel(message: message) else { break }
            let sessionID = Int(Wire.u32(message, "session_id"))
            if var row = rows[sessionID] {
                if model.focus != 0 { row.activePane = model.focus }
                rows[sessionID] = row
            }
            guard sessionID == selectedSession else { break }
            if model.focus != 0 { focusedPane = model.focus }
            container.apply(model)
            flushPendingSnapshots()
            for pane in model.panes.map(\.id) { sendAttach(pane) }
        case PG_MSG_SNAPSHOT:
            guard let snapshot = pg_msg_snapshot(message) else { break }
            defer { pg_snapshot_free(snapshot) }
            let pane = snapshot.pointee.pane_id
            focusedPane = focusedPane ?? pane
            if container.applySnapshot(snapshot) {
                haveSnapshot.insert(pane)
            } else if let frame = Wire.frame(message) {
                pendingSnapshots[pane] = frame
            }
        case PG_MSG_STATUS:
            applyStatus(message)
        case PG_MSG_PANE_EXITED:
            break
        case PG_MSG_HELLO_ERROR:
            NSLog("pigeon: handshake rejected: \(Wire.string(message, "detail"))")
        case PG_MSG_ERROR:
            NSLog("pigeon: \(Wire.string(message, "code")): \(Wire.string(message, "detail"))")
        default:
            break
        }
    }

    private func applySessions(_ message: OpaquePointer) {
        let count = pg_msg_sessions_count(message)
        var updated: [Int: Row] = [:]
        for index in 0..<count {
            var info = PgSessionInfo()
            guard pg_msg_session_at(message, index, &info), info.pane_count > 0 else { continue }
            var panes = [UInt32](repeating: 0, count: Int(info.pane_count))
            let written = panes.withUnsafeMutableBufferPointer { buffer in
                pg_msg_session_panes(message, index, buffer.baseAddress, buffer.count)
            }
            guard written > 0 else { continue }
            let id = Int(info.session_id)
            let previous = rows[id]
            updated[id] = Row(
                id: id,
                panes: Array(panes.prefix(written)),
                cwd: Wire.string(info.cwd),
                activePane: info.active_pane != 0 ? info.active_pane : (previous?.activePane ?? panes[0]),
                app: info.active_pane != 0 ? Wire.string(info.active_title) : (previous?.app ?? "shell"),
                title: "session \(id)",
                status: previous?.status ?? .idle,
                seq: previous?.seq ?? 0,
                unread: previous?.unread ?? state.unread[String(id)] ?? 0,
                lastSeen: previous?.lastSeen ?? Date()
            )
        }
        rows = updated
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let apps = rows.values.map { "\($0.id):\($0.app)" }.sorted().joined(separator: " ")
            FileHandle.standardError.write(Data("pigeon: sessions apps=\(apps)\n".utf8))
        }
        if selectedSession == nil || rows[selectedSession!] == nil {
            selectedSession = rows.keys.sorted().first
        }
        // Watch the active pane of *every* session: the sidebar shows their
        // status even when they are not selected.
        for row in rows.values { sendAttach(row.activePane) }
        if let selectedSession, let row = rows[selectedSession] {
            focusedPane = row.activePane
            for pane in row.panes { sendAttach(pane) }
        }
        refreshSidebar()
    }

    private func applyStatus(_ message: OpaquePointer) {
        let pane = Wire.u32(message, "pane_id")
        guard let id = rows.first(where: { $0.value.panes.contains(pane) })?.key,
              var row = rows[id] else { return }
        if let status = SidebarModel.status(from: Wire.string(message, "state")) {
            row.status = status
        }
        row.seq = Wire.u32(message, "seq")
        row.lastSeen = Date()
        if selectedSession != id && row.status != .idle {
            row.unread += 1
            state.unread[String(id)] = row.unread
            store.save(state)
        }
        rows[id] = row
        if selectedSession == id {
            sendAck(row.activePane, row.seq)
        }
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
                pendingSnapshots.removeValue(forKey: pane)
            }
        }
    }
}
