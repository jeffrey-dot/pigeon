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
        /// Who owns the size when more than one window watches this session (protocol §6).
        var sizeSource: String = ""
        var clients: Int = 1
        /// Where the session lives; `local` for the host on this machine.
        var host: String = "local"

        var group: String {
            guard !cwd.isEmpty else { return "~" }
            return Grouping.groupName(forCwd: cwd, gitRoot: GitRoot.find(cwd: cwd))
        }
    }

    private var window: NSWindow!
    private var splitView: NSSplitView!
    private var sidebar: SidebarViewController!
    private var header: SessionHeaderView!
    private var container: SplitContainerView!
    private var hostStatus: HostStatusView!
    private var client: HostClient?
    /// Set when `PIGEON_REMOTE=<host>` picked a remote machine instead of the local socket.
    private var remote: RemoteSession?
    /// Mode C keeps the VT state here (no host on the far side).
    private var localSession: LocalSession?
    /// Attempts made since the socket last worked; drives `HostRecovery`.
    private var recoveryAttempt = 0
    /// Why the host rejected this client (bad version, busy); retrying cannot help.
    private var refusedReason: String?

    private var rows: [Int: Row] = [:]
    private var selectedSession: Int?
    private var focusedPane: UInt32?
    private var requested: Set<UInt32> = []
    private var haveSnapshot: Set<UInt32> = []
    private var pendingSnapshots: [UInt32: Data] = [:]
    private var nextRequest: UInt32 = 1
    /// Session created by `+` / ⌘T: select it once the refreshed list contains it.
    private var pendingSelectSession: Int?
    /// First launch on an empty host: give the user one session instead of an empty window.
    private var createdInitialSession = false

    private var appearanceObservation: NSKeyValueObservation?
    private let store = SidebarStore()
    private var state = SidebarState()
    private let hostsStore = HostsStore()
    private var hosts = HostsState()
    /// Set while a remote probe runs, so the sidebar row says `connecting` rather than
    /// `offline` (docs/remote.md §4 asks for the status on the host, not only in a banner).
    private var connectingHost: String?

    func applicationDidFinishLaunching(_ notification: Notification) {
        // `PIGEON_APPEARANCE=light|dark|system` pins the app's appearance for the screenshot
        // runs (`tools/m3-gui-check.sh`). Pinning sets `NSApp.appearance`, which overrides the
        // system until it is cleared again — hence the third value and the three-way toggle.
        NSApp.appearance = AppearanceMode
            .fromEnvironment(ProcessInfo.processInfo.environment["PIGEON_APPEARANCE"]).appearance
        // The system flipping light/dark is another way the palette changes. AppKit does not
        // post a notification for it, but `effectiveAppearance` is KVO-observable.
        appearanceObservation = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
            self?.sendAppearance()
        }
        state = store.load()
        hosts = hostsStore.load()
        buildMenu()
        buildWindow()
        if let host = ProcessInfo.processInfo.environment["PIGEON_REMOTE"], !host.isEmpty {
            startRemote(host)
        } else if let remembered = hosts.current {
            // The window reopens where it was left (`docs/remote.md` §4).
            startRemote(remembered)
        } else if !connectHost() {
            hostLost("No session host is running.")
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    // MARK: - window

    private func buildWindow() {
        // No hardcoded origin: the first launch centres on screen, later launches restore the
        // frame the user left behind (`setFrameAutosaveName`).
        let frame = NSRect(x: 0, y: 0, width: 1100, height: 600)
        window = NSWindow(
            contentRect: frame,
            styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "pigeon"
        // `docs/ui.md` §7.1: the sidebar runs under a transparent titlebar, with the traffic
        // lights sitting inside its 56pt header (harp: TL:64, VT:9112).
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isMovableByWindowBackground = true

        sidebar = SidebarViewController()
        sidebar.onSelect = { [weak self] id in self?.select(id) }
        sidebar.onClose = { [weak self] id in self?.closeSession(id) }
        sidebar.onMarkRead = { [weak self] id in self?.markRead(id) }
        sidebar.onRename = { [weak self] id, name in self?.rename(id, name) }
        sidebar.onRenameGroup = { [weak self] group, name in self?.renameGroup(group, name) }
        sidebar.onReorder = { [weak self] order in self?.reorder(order) }
        sidebar.onNewSession = { [weak self] cwd in self?.newSession(cwd.isEmpty ? nil : cwd) }
        sidebar.onToggleSidebar = { [weak self] in self?.toggleSidebar(nil) }

        container = SplitContainerView(frame: frame)
        container.onSplit = { [weak self] axis in self?.split(axis) }
        container.onClose = { [weak self] in self?.closeFocusedPane() }
        container.onFocus = { [weak self] pane in self?.focus(pane) }
        container.onResizeSplit = { [weak self] split, ratio in self?.resizeSplit(split, ratio) }
        container.onPaneInput = { [weak self] pane, bytes in self?.sendKeys(pane, bytes) }
        container.onPaneResize = { [weak self] pane, cols, rows in
            self?.sendResize(pane, cols: cols, rows: rows)
        }
        // Scrollback lives in the host; the window only says which way to look.
        container.onPaneScroll = { [weak self] pane, lines in self?.sendScroll(pane, lines: lines) }
        container.onPaneScrollBottom = { [weak self] pane in self?.sendScroll(pane, lines: 0) }
        container.onToggleZoom = { [weak self] in self?.toggleZoom() }

        splitView = NSSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin
        // `docs/ui.md` §7.1: harp's 272pt by default, user drag overrides (low priority).
        let defaultWidth = sidebar.view.widthAnchor.constraint(equalToConstant: SidebarMetrics.width)
        defaultWidth.priority = .defaultLow
        defaultWidth.isActive = true
        header = SessionHeaderView()
        header.translatesAutoresizingMaskIntoConstraints = false
        header.heightAnchor.constraint(equalToConstant: SidebarMetrics.cardHeaderHeight).isActive = true
        // The card: header on top, terminal below, one rounded surface inset from the window.
        let card = CardView()
        // harp pads the terminal inside the card 32pt from the left, under the header's name
        // (`FRAMED_CARD_CONTENT_LEFT_PADDING`), so tmux text lines up with the title above it.
        let contentBox = NSView()
        contentBox.addSubview(container)
        container.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            container.leadingAnchor.constraint(
                equalTo: contentBox.leadingAnchor, constant: SidebarMetrics.cardContentLeftPadding),
            container.topAnchor.constraint(equalTo: contentBox.topAnchor),
            container.trailingAnchor.constraint(equalTo: contentBox.trailingAnchor),
            container.bottomAnchor.constraint(equalTo: contentBox.bottomAnchor),
        ])
        // Explicit constraints instead of a stack view: the content box has no intrinsic
        // height, so a vertical stack gave it zero and the terminal never got a size.
        hostStatus = HostStatusView()
        hostStatus.onAction = { [weak self] in self?.retryHost() }
        contentBox.addSubview(hostStatus)
        hostStatus.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            hostStatus.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            hostStatus.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            hostStatus.topAnchor.constraint(equalTo: container.topAnchor),
            hostStatus.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
        card.addSubview(header)
        card.addSubview(contentBox)
        contentBox.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            header.topAnchor.constraint(equalTo: card.topAnchor),
            header.leadingAnchor.constraint(equalTo: card.leadingAnchor),
            header.trailingAnchor.constraint(equalTo: card.trailingAnchor),
            contentBox.topAnchor.constraint(equalTo: header.bottomAnchor),
            contentBox.leadingAnchor.constraint(equalTo: card.leadingAnchor),
            contentBox.trailingAnchor.constraint(equalTo: card.trailingAnchor),
            contentBox.bottomAnchor.constraint(equalTo: card.bottomAnchor),
        ])
        // The gutter around the card is the sidebar's surface, so the card looks lifted.
        let gutter = SidebarBackgroundView()
        gutter.addSubview(card)
        card.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            card.topAnchor.constraint(equalTo: gutter.topAnchor, constant: SidebarMetrics.cardInset),
            card.trailingAnchor.constraint(equalTo: gutter.trailingAnchor, constant: -SidebarMetrics.cardInset),
            card.bottomAnchor.constraint(equalTo: gutter.bottomAnchor, constant: -SidebarMetrics.cardInset),
            card.leadingAnchor.constraint(equalTo: gutter.leadingAnchor),
        ])
        splitView.addArrangedSubview(sidebar.view)
        splitView.addArrangedSubview(gutter)
        sidebar.view.widthAnchor.constraint(
            greaterThanOrEqualToConstant: SidebarMetrics.widthRange.lowerBound).isActive = true
        sidebar.view.widthAnchor.constraint(
            lessThanOrEqualToConstant: SidebarMetrics.widthRange.upperBound).isActive = true

        window.contentView = splitView
        if !window.setFrameUsingName("pigeon.main") {
            window.center()
        }
        window.setFrameAutosaveName("pigeon.main")
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(container)
        NSApp.activate(ignoringOtherApps: true)
        // `NSSplitView` sizes panes by frames, so the default width needs a real position;
        // the low-priority width constraint alone only nudges autolayout.
        splitView.setPosition(SidebarMetrics.width, ofDividerAt: 0)
        alignSidebarHeaderWithTrafficLights()
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data(
                "pigeon: sidebar width=\(Int(sidebar.view.frame.width)) window=\(Int(window.frame.width))\n".utf8
            ))
        }
        refreshSidebar()
    }

    /// harp puts the sidebar's chips on the traffic lights' row; the native buttons live in
    /// the titlebar overlay, so their center is the only reliable reference.
    private func alignSidebarHeaderWithTrafficLights() {
        guard let zoom = window.standardWindowButton(.zoomButton) else { return }
        let center = zoom.convert(NSPoint(x: zoom.bounds.midX, y: zoom.bounds.midY), to: nil)
        sidebar.alignHeaderControls(centerFromTop: window.frame.height - center.y)
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
        let toggleSidebarItem = NSMenuItem(
            title: "Toggle Sidebar", action: #selector(toggleSidebar(_:)), keyEquivalent: "b")
        toggleSidebarItem.target = self
        viewMenu.addItem(toggleSidebarItem)
        viewMenu.addItem(withTitle: "Close Pane", action: #selector(SplitContainerView.closeFocused(_:)), keyEquivalent: "w")
        viewMenu.addItem(SplitContainerView.zoomMenuItem())
        viewMenu.addItem(withTitle: "Previous Pane", action: #selector(SplitContainerView.focusPrevious(_:)), keyEquivalent: "[")
        viewMenu.addItem(withTitle: "Next Pane", action: #selector(SplitContainerView.focusNext(_:)), keyEquivalent: "]")
        viewItem.submenu = viewMenu

        let sessionItem = NSMenuItem()
        mainMenu.addItem(sessionItem)
        let sessionMenu = NSMenu(title: "Session")
        sessionMenu.addItem(withTitle: "Next Needs You", action: #selector(nextNeedsYou(_:)), keyEquivalent: "j")
        sessionMenu.addItem(withTitle: "New Session", action: #selector(newSession(_:)), keyEquivalent: "t")
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            // Screenshot runs flip the appearance inside one process, which is the same path
            // AppKit takes when the user switches the system appearance.
            let toggle = NSMenuItem(
                title: "Toggle Appearance", action: #selector(toggleAppearance(_:)), keyEquivalent: "a")
            toggle.keyEquivalentModifierMask = [.command, .option]
            toggle.target = self
            sessionMenu.addItem(toggle)
        }
        let close = NSMenuItem(title: "Close Session", action: #selector(closeSelectedSession(_:)), keyEquivalent: "W")
        sessionMenu.addItem(close)
        for item in sessionMenu.items { item.target = self }
        sessionItem.submenu = sessionMenu

        let hostsItem = NSMenuItem()
        mainMenu.addItem(hostsItem)
        let hostsMenu = NSMenu(title: "Hosts")
        for host in hosts.hosts {
            let item = NSMenuItem(
                title: host, action: #selector(selectHost(_:)), keyEquivalent: "")
            item.representedObject = host
            item.state = hosts.current == host ? .on : .off
            hostsMenu.addItem(item)
        }
        let local = NSMenuItem(
            title: "Use This Mac", action: #selector(useThisMac(_:)), keyEquivalent: "")
        local.state = hosts.current == nil ? .on : .off
        hostsMenu.addItem(local)
        hostsMenu.addItem(.separator())
        let add = NSMenuItem(
            title: "Connect to Host…", action: #selector(connectToHost(_:)), keyEquivalent: "o")
        add.keyEquivalentModifierMask = [.command, .shift]
        hostsMenu.addItem(add)
        for item in hostsMenu.items { item.target = self }
        hostsItem.submenu = hostsMenu
        NSApp.mainMenu = mainMenu
    }

    // MARK: - host

    /// `PIGEON_REMOTE=<ssh host>`: open a remote session instead of the local socket
    /// (`docs/remote.md` §1). Probing runs off the main thread — ssh takes a second and the
    /// window must appear first.
    private func startRemote(_ host: String) {
        // Remember the machine even when it was reached through `PIGEON_REMOTE` or the
        // remembered-host path: otherwise the Machines list would not show it at all.
        if !hosts.hosts.contains(host) {
            hosts.hosts.append(host)
        }
        if hosts.current != host {
            hosts.current = host
            hostsStore.save(hosts)
        }
        let session = RemoteSession(host: host)
        remote = session
        connectingHost = host
        refreshSidebar()
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let outcome = try session.connect()
                DispatchQueue.main.async { [weak self] in self?.apply(outcome) }
            } catch {
                DispatchQueue.main.async { [weak self] in
                    self?.connectingHost = nil
                    self?.refreshSidebar()
                    self?.showStopped("cannot reach \(host): \(error)")
                }
            }
        }
    }

    private func apply(_ outcome: RemoteSession.Outcome) {
        debugLog("remote transport=\(outcome.transport.rawValue) host=\(remote?.host ?? "-")")
        connectingHost = nil
        refreshSidebar()
        if let client = outcome.client {
            wire(client)
            return
        }
        guard let process = outcome.process, let host = remote?.host else { return }
        startDegraded(process: process, host: host, note: outcome.note)
    }

    /// Mode C: plain ssh, local VT. The window says plainly that nothing outlives it.
    private func startDegraded(process: Process, host: String, note: String) {
        let session = LocalSession()
        localSession = session
        container.onPaneInput = { [weak session] _, bytes in session?.write(bytes) }
        container.onPaneResize = { [weak session] _, cols, rows in
            session?.resize(cols: cols, rows: rows)
        }
        session.onUpdate = { [weak self] in
            DispatchQueue.main.async { self?.applyLocalSnapshot() }
        }
        let model = LayoutModel(
            panes: [LayoutModel.Pane(
                id: session.pane, rect: CGRect(x: 0, y: 0, width: 1, height: 1))],
            dividers: [],
            focus: session.pane
        )
        container.apply(model)
        session.attach(process: process)
        window.title = "pigeon — \(host) (not persistent)"
        header.update(title: host, cwd: note, badge: "Not persistent")
        hostStatus.hide()
    }

    private func applyLocalSnapshot() {
        guard let session = localSession, let snapshot = session.snapshot() else { return }
        defer { pg_snapshot_free(snapshot) }
        _ = container.applySnapshot(snapshot)
    }

    /// Tries to connect; `false` when the socket is not there (yet).
    @discardableResult
    private func connectHost() -> Bool {
        let path = Self.defaultSocketPath()
        do {
            let client = try HostClient.connect(path: path)
            wire(client)
            return true
        } catch {
            debugLog("host unreachable at \(path): \(error)")
            return false
        }
    }

    /// Points the window at a live client: the local socket and a remote bridge look the same
    /// from here on (that is the point of mode A/B).
    private func wire(_ client: HostClient) {
        client.onMessage = { [weak self] message in
            DispatchQueue.main.async {
                defer { Wire.free(message) }
                self?.handle(message)
            }
        }
        client.onClose = { [weak self] in
            DispatchQueue.main.async { self?.hostLost("The session host closed the connection.") }
        }
        self.client = client
        recoveryAttempt = 0
        hostStatus.hide()
        sendHello()
    }

    /// The socket is gone: say so, then try to bring it back (including starting the host) a
    /// few times before asking the user to.
    private func hostLost(_ reason: String) {
        client = nil
        if let refused = refusedReason {
            refusedReason = nil
            showStopped(refused)
            return
        }
        recoveryAttempt = 0
        recover(reason)
    }

    private func recover(_ reason: String) {
        let remote = self.remote != nil
        let allowed = remote
            ? HostRecovery.canRetryRemote(after: recoveryAttempt)
            : HostRecovery.canRetry(after: recoveryAttempt)
        guard allowed else {
            showStopped(reason)
            return
        }
        recoveryAttempt += 1
        show(hostStatus: .reconnecting(attempt: recoveryAttempt))
        let delay = remote ? HostRecovery.remoteRetryDelay : HostRecovery.retryDelay
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self else { return }
            if remote {
                self.reconnectRemote(reason)
                return
            }
            guard !self.connectHost() else { return }
            // No socket at all: start a local host and give it a moment to bind.
            if let path = HostLauncher.find(candidates: HostLauncher.candidates()) {
                HostLauncher.launch(path: path)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + HostRecovery.retryDelay) { [weak self] in
                guard let self, !self.connectHost() else { return }
                self.recover(reason)
            }
        }
    }

    /// A dropped ssh channel comes back through the same A→B→C probe. The remote host kept
    /// the panes, which is what makes a 30 s drop survivable at all.
    private func reconnectRemote(_ reason: String) {
        guard let session = remote else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            do {
                // Never silently fall back to plain ssh here: that would replace the
                // persistent session with one that dies with the window.
                let outcome = try session.connect(allowDegraded: false)
                DispatchQueue.main.async { self?.apply(outcome) }
            } catch {
                DispatchQueue.main.async { [weak self] in
                    self?.recover("\(reason) (\(error))")
                }
            }
        }
    }

    private func showStopped(_ reason: String) {
        if let remote {
            show(hostStatus: .stopped(
                detail: "\(reason)\nCheck `ssh \(remote.host)`, then Retry."))
            return
        }
        show(hostStatus: .stopped(detail: "\(reason)\nStart it with: \(HostRecovery.startCommand)"))
    }

    private func show(hostStatus state: HostRecovery.State) {
        guard let text = HostRecovery.banner(for: state) else {
            hostStatus.hide()
            return
        }
        hostStatus.show(message: text, action: HostRecovery.action(for: state))
    }

    private func retryHost() {
        recoveryAttempt = 0
        if let remote {
            recover("Reconnecting to \(remote.host)")
            return
        }
        if !connectHost() {
            recover("No session host is running.")
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

    /// Reports the palette the panes should answer colour queries with. Sent after the
    /// handshake and again whenever the appearance changes.
    private func sendAppearance() {
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

    private func sendHello() {
        send({ message in
            Wire.set(message, u32: "proto", 0x0100)
            Wire.set(message, str: "client", "pigeon-mac")
            Wire.set(message, strings: "features", ["scrollback"])
        }, type: PG_MSG_HELLO)
    }

    private func debugLog(_ text: String) {
        guard ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil else { return }
        FileHandle.standardError.write(Data("pigeon: \(text)\n".utf8))
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

    /// Attaching is also how the host learns which session this window follows: it switches
    /// `state.session` to that session and replies with its layout + snapshot. The startup pass
    /// attaches every session (for the sidebar's status), so selecting a session has to force
    /// a re-attach — otherwise the host keeps following whichever session it attached last and
    /// the window never receives the selected session's layout.
    private func sendAttach(_ pane: UInt32, force: Bool = false) {
        guard pane != 0 else { return }
        if !force, haveSnapshot.contains(pane) || !requested.insert(pane).inserted { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", pane)
        }, type: PG_MSG_ATTACH)
    }

    /// `scroll` takes a delta: negative looks back, `0` is the live screen (protocol §3.3).
    private func sendScroll(_ pane: UInt32, lines: Int) {
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
            Wire.set(message, i32: "offset", -Int32(lines))
        }, type: PG_MSG_SCROLL)
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
        if id < 0 {
            // A `Machines` row: switch to that host (or back to this Mac).
            guard let target = SidebarModel.host(forRow: id, hosts: hosts.hosts) else { return }
            switchTo(host: target)
            return
        }
        guard let row = rows[id] else { return }
        debugLog("select id=\(id) activePane=\(row.activePane) panes=\(row.panes)")
        selectedSession = id
        focusedPane = row.activePane
        for pane in row.panes { sendAttach(pane, force: true) }
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

    // MARK: - hosts

    @objc private func selectHost(_ sender: NSMenuItem) {
        guard let host = sender.representedObject as? String else { return }
        switchTo(host: host)
    }

    @objc private func useThisMac(_ sender: Any?) {
        switchTo(host: nil)
    }

    @objc private func connectToHost(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = "Connect to host"
        alert.informativeText =
            "An ssh destination, e.g. `arch`. pigeon-host is installed there on first use."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.placeholderString = "user@host"
        alert.accessoryView = field
        alert.addButton(withTitle: "Connect")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let host = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !host.isEmpty else { return }
        switchTo(host: host)
    }

    /// One window is one connection, so switching hosts means hanging up first — and saying
    /// so by clearing the old rows instead of half-showing them.
    private func switchTo(host: String?) {
        debugLog("host switch to \(host ?? "local")")
        client?.close()
        client = nil
        remote?.close()
        remote = nil
        localSession = nil
        rows = [:]
        haveSnapshot = []
        requested = []
        selectedSession = nil
        pendingSnapshots = [:]
        container.apply(.empty)
        if let host {
            if !hosts.hosts.contains(host) {
                hosts.hosts.append(host)
            }
            hosts.current = host
        } else {
            hosts.current = nil
        }
        hostsStore.save(hosts)
        buildMenu()
        refreshSidebar()
        if let host {
            startRemote(host)
        } else if !connectHost() {
            hostLost("No session host is running.")
        }
    }

    @objc private func nextNeedsYou(_ sender: Any?) {
        let sessions = sidebarSessions()
        guard let next = SidebarModel.nextNeedsYou(sessions, after: selectedSession) else { return }
        select(next.id)
    }

    @objc private func newSession(_ sender: Any?) {
        sendNewSession(cwd: rows[selectedSession ?? -1]?.cwd)
    }

    /// ⌘⇧↩ zooms the focused pane; pressing it again gives the siblings their space back.
    private func toggleZoom() {
        guard let type = SplitContainerView.zoomRequest(
            currentZoom: container.model.zoom, focused: focusedPane ?? 0)
        else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", focusedPane ?? 0)
        }, type: type)
    }

    @objc private func closeSelectedSession(_ sender: Any?) {
        guard let selectedSession else { return }
        closeSession(selectedSession)
    }

    private func sidebarSessions() -> [SidebarSession] {
        // Machines first: one row per known host, so "where does this session live" and
        // "switch to that machine" are the same list the sessions are grouped in.
        var sessions = SidebarModel.hostRows(
            hosts: hosts.hosts,
            connected: hosts.current,
            connecting: connectingHost)
        // Dictionary order is random; sort by id so groups keep first-seen order across launches.
        sessions += rows.values.sorted { $0.id < $1.id }.map { row in
            SidebarSession(
                id: row.id,
                title: row.app,
                group: row.group,
                status: row.status,
                unread: row.unread,
                app: row.app,
                cwd: row.cwd,
                lastSeen: row.lastSeen,
                reported: row.seq > 0,
                sizeSource: row.sizeSource,
                clients: row.clients,
                host: row.host
            )
        }
        return sessions
    }

    private func refreshSidebar() {
        let raw = SidebarModel.sections(sidebarSessions(), order: state.order)
        let sections = raw.map { section in
            (group: state.renamedGroups[section.group] ?? section.group, sessions: section.sessions)
        }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let needs = SidebarModel.needsYou(sidebarSessions())
            let machines = sidebarSessions()
                .filter { $0.group == SidebarModel.machinesGroup }
                .map { "\($0.title)=\($0.statusText ?? "-")" }
                .joined(separator: ",")
            debugLog(
                "sidebar sessions=\(rows.count) needs=\(needs) "
                    + "selected=\(selectedSession.map(String.init) ?? "-") "
                    + "groups=\(sections.map(\.group).joined(separator: "|")) "
                    + "machines=\(machines)")
        }
        sidebar.update(sections: sections, selected: selectedSession)
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
        header.update(
            title: row.app,
            cwd: cwd,
            // No OSC 7501 record means no state to report: leave the badge blank.
            badge: row.seq > 0
                ? SidebarModel.subtitle(status: row.status, since: row.lastSeen)
                : ""
        )
    }

    /// light → dark → follow the system. Clearing `NSApp.appearance` hands control back to
    /// the system, which is the only way to undo a pin without relaunching.
    @objc private func toggleAppearance(_ sender: Any?) {
        let mode = AppearanceMode.current().next
        NSApp.appearance = mode.appearance
        debugLog("appearance -> \(mode.rawValue)")
        // No explicit send here: the `effectiveAppearance` observer fires on this change too,
        // and sending twice would just duplicate the message.
    }

    @objc private func toggleSidebar(_ sender: Any?) {
        sidebar.view.isHidden.toggle()
        // A hidden arranged subview comes back at width 0, which looks like "cannot reopen".
        if !sidebar.view.isHidden {
            splitView.setPosition(SidebarMetrics.width, ofDividerAt: 0)
        }
        // With no sidebar the traffic lights sit over the right pane, so its header
        // pads past them (harp's `CARD_HEADER_PAD_CLOSED`).
        header.setSidebarCollapsed(sidebar.view.isHidden)
    }

    // MARK: - protocol

    private func handle(_ message: OpaquePointer) {
        switch Wire.type(message) {
        case PG_MSG_HELLO_ACK:
            // Tell the host what we are drawing with, so programs inside the panes can pick
            // their light or dark theme (`OSC 4/10/11`).
            sendAppearance()
        case PG_MSG_SESSIONS:
            applySessions(message)
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
                // `scroll` answers carry where the view is; plain snapshots do not, and then
                // the indicator stays hidden (both fields are 0).
                container.paneView(pane)?.setScrollPosition(
                    total: Int(Wire.u32(message, "history_len")),
                    offset: Int(Wire.u32(message, "offset")))
            } else if let frame = Wire.frame(message) {
                pendingSnapshots[pane] = frame
            }
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
            // `close_pane`, so dropping the row locally is not enough — closing the session is
            // what makes the host forget it, otherwise the next `sessions` refresh re-adds it.
            let pane = Wire.u32(message, "pane_id")
            guard let id = rows.first(where: { $0.value.panes.contains(pane) })?.key else { break }
            debugLog("pane exited pane=\(pane) -> closing session \(id)")
            closeSession(id)
        case PG_MSG_HELLO_ERROR:
            // Retrying a refused handshake only repeats it, so stop and explain.
            refusedReason =
                "The host refused this client (\(Wire.string(message, "reason")): \(Wire.string(message, "detail")))."
            client?.close()
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
                lastSeen: previous?.lastSeen ?? Date(),
                sizeSource: previous?.sizeSource ?? "",
                clients: previous?.clients ?? 1,
                host: previous?.host ?? remote?.host ?? "local"
            )
        }
        rows = updated
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let apps = rows.values.map { "\($0.id):\($0.app)" }.sorted().joined(separator: " ")
            FileHandle.standardError.write(Data("pigeon: sessions apps=\(apps)\n".utf8))
        }
        if rows.isEmpty, !createdInitialSession {
            // First launch on an empty host: one session instead of an empty window.
            createdInitialSession = true
            debugLog("no sessions on the host: creating one")
            sendNewSession(cwd: nil)
            return
        }
        if let pending = pendingSelectSession, rows[pending] != nil {
            pendingSelectSession = nil
            select(pending)
            return
        }
        if selectedSession == nil || rows[selectedSession!] == nil {
            selectedSession = rows.keys.sorted().first
        }
        // Watch the active pane of *every* session: the sidebar shows their
        // status even when they are not selected.
        for row in rows.values { sendAttach(row.activePane) }
        if let selectedSession, let row = rows[selectedSession] {
            focusedPane = row.activePane
            // Force the selected session's panes: `attach` is what makes the host follow this
            // session and reply with its layout + snapshot. Without it a fresh launch shows an
            // empty pane until the first click (which selects the already-selected row).
            for pane in row.panes { sendAttach(pane, force: true) }
            send({ message in Wire.set(message, u32: "pane_id", row.activePane) }, type: PG_MSG_FOCUS)
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
        // `docs/ui.md` §5: only done/error become unread; blocked is its own orange mark.
        if selectedSession != id, row.status == .done || row.status == .error {
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
                // `scroll` answers carry where the view is; plain snapshots do not, and then
                // the indicator stays hidden (both fields are 0).
                container.paneView(pane)?.setScrollPosition(
                    total: Int(Wire.u32(decoded, "history_len")),
                    offset: Int(Wire.u32(decoded, "offset")))
                pendingSnapshots.removeValue(forKey: pane)
            }
        }
    }
}
