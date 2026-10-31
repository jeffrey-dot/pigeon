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
        /// The user's group, fixed when the session first appeared (`Grouping.pinnedGroup`):
        /// a `cd` later must not reshuffle the sidebar.
        var group: String = ""
    }

    private var window: NSWindow!
    private var splitView: NSSplitView!
    private var sidebar: SidebarViewController!
    private var header: SessionHeaderView!
    private var container: SplitContainerView!
    private var hostStatus: HostStatusView!
    /// One live connection per machine (`docs/remote.md` §4). `"local"` is this Mac.
    private var clients: [String: HostClient] = [:]
    private var remotes: [String: RemoteSession] = [:]
    private var rowsByHost: [String: [Int: Row]] = [:]
    private var focusedByHost: [String: UInt32] = [:]
    private var seenByHost: [String: Set<UInt32>] = [:]
    private var requestedByHost: [String: Set<UInt32>] = [:]
    private var pendingByHost: [String: [UInt32: Data]] = [:]
    /// Last frame `seq` drawn per pane: a `delta` must be exactly the next one, otherwise a
    /// frame went missing and the pane re-attaches for a snapshot (protocol §3.3).
    private var seqByHost: [String: [UInt32: UInt32]] = [:]
    /// Panes still showing what a dropped connection drew: their next `attach` carries
    /// `since` so the host replays only the missed deltas (issue #4).
    private var staleByHost: [String: Set<UInt32>] = [:]
    /// `hello_ack.instance` per host; `since` is only meaningful to that same process.
    private var instanceByHost: [String: String] = [:]
    private var recovering: [String: Int] = [:]
    private let hostAgent = HostAgent()
    /// Set by "Stop Session Host…" so the dropped socket is not taken for a crash.
    private var stoppedByUser = false
    /// The host whose layout is on screen; every host stays connected in the background.
    private var activeHost = "local"
    /// Stable order for namespaced sidebar ids (`host index * 100_000 + session`).
    private var hostOrder: [String] = ["local"]

    private var client: HostClient? {
        get { clients[activeHost] }
        set { clients[activeHost] = newValue }
    }
    private var remote: RemoteSession? {
        get { remotes[activeHost] }
        set { remotes[activeHost] = newValue }
    }
    private var rows: [Int: Row] {
        get { rowsByHost[activeHost] ?? [:] }
        set { rowsByHost[activeHost] = newValue }
    }
    private var focusedPane: UInt32? {
        get { focusedByHost[activeHost] }
        set { focusedByHost[activeHost] = newValue }
    }
    private var haveSnapshot: Set<UInt32> {
        get { seenByHost[activeHost] ?? [] }
        set { seenByHost[activeHost] = newValue }
    }
    private var requested: Set<UInt32> {
        get { requestedByHost[activeHost] ?? [] }
        set { requestedByHost[activeHost] = newValue }
    }
    private var pendingSnapshots: [UInt32: Data] {
        get { pendingByHost[activeHost] ?? [:] }
        set { pendingByHost[activeHost] = newValue }
    }
    private var frameSeq: [UInt32: UInt32] {
        get { seqByHost[activeHost] ?? [:] }
        set { seqByHost[activeHost] = newValue }
    }
    /// Mode C keeps the VT state here (no host on the far side).
    private var localSession: LocalSession?
    /// Why the host rejected this client (bad version, busy); retrying cannot help.
    private var refusedReason: String?
    /// Messages this window has sent, by type: what the self-test asserts on.
    private var sentCounts: [PgMsgType: Int] = [:]
    /// `PIGEON_SELFTEST=1` drives the real menu/keyboard/sidebar paths once and exits.
    private let selfTest = ProcessInfo.processInfo.environment["PIGEON_SELFTEST"] == "1"
    private var selfTestRunning = false
    /// The last row the sidebar asked to select (a `Machines` row uses a negative id).
    private var lastSelectedId: Int?

    private var selectedByHost: [String: Int] = [:]
    private var selectedSession: Int? {
        get { selectedByHost[activeHost] }
        set { selectedByHost[activeHost] = newValue }
    }
    private var nextRequest: UInt32 = 1
    /// Sessions per host, and which host is the one on screen. Every host keeps its own
    /// connection (`docs/remote.md` §4: one ssh per machine, N sessions on it).
    /// Session created by `+` / ⌘T: select it once the refreshed list contains it.
    private var pendingSelectSession: Int?
    /// Hosts that already got their "empty host, one session" treatment.
    private var createdInitialSessionFor: Set<String> = []

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
            startRemote(host, select: true)
            return
        }
        // Every known machine connects at launch (`docs/remote.md` §4): the sidebar lists
        // their sessions side by side, not one at a time.
        activeHost = hosts.current ?? "local"
        if !connectHost() {
            hostLost(host: "local", "No session host is running.")
        }
        for host in hosts.hosts {
            startRemote(host)
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
        // Quitting leaves the host and its sessions running; ending them is its own command.
        let stopHost = NSMenuItem(
            title: "Stop Session Host…", action: #selector(stopLocalHost(_:)), keyEquivalent: "")
        stopHost.target = self
        appMenu.addItem(stopHost)
        appMenu.addItem(.separator())
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

    /// Connects to one machine (`docs/remote.md` §4: every known host gets its own ssh
    /// connection; the window only shows the one whose session is selected).
    private func startRemote(_ host: String, select: Bool = false) {
        if !hosts.hosts.contains(host) {
            hosts.hosts.append(host)
            hostsStore.save(hosts)
        }
        connectingHost = host
        if select {
            activeHost = host
        }
        refreshSidebar()
        let session = remotes[host] ?? RemoteSession(host: host)
        remotes[host] = session
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let outcome = try session.connect()
                DispatchQueue.main.async { [weak self] in self?.apply(outcome, host: host) }
            } catch {
                DispatchQueue.main.async { [weak self] in
                    self?.connectingHost = nil
                    self?.refreshSidebar()
                    self?.debugLog("cannot reach \(host): \(error)")
                }
            }
        }
    }

    private func apply(_ outcome: RemoteSession.Outcome, host: String) {
        debugLog("remote transport=\(outcome.transport.rawValue) host=\(host)")
        connectingHost = nil
        refreshSidebar()
        if let client = outcome.client {
            wire(client, host: host)
            return
        }
        guard let process = outcome.process else { return }
        // Degraded (mode C) owns the terminal area, so it only makes sense for the host the
        // window is showing; a background host just stays offline until it is selected.
        guard host == activeHost else {
            debugLog("degraded host \(host) not visible; leaving it offline")
            refreshSidebar()
            return
        }
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
            wire(client, host: "local")
            return true
        } catch {
            debugLog("host unreachable at \(path): \(error)")
            return false
        }
    }

    /// Points the window at a live client: the local socket and a remote bridge look the same
    /// from here on (that is the point of mode A/B).
    private func wire(_ client: HostClient, host: String) {
        client.onMessage = { [weak self] message in
            DispatchQueue.main.async {
                defer { Wire.free(message) }
                self?.handle(message, host: host)
            }
        }
        client.onClose = { [weak self] in
            DispatchQueue.main.async {
                self?.hostLost(host: host, "The session host closed the connection.")
            }
        }
        clients[host] = client
        recovering[host] = 0
        // A reconnect keeps the screens on display; every pane drawn so far re-attaches,
        // asking only for what it missed.
        staleByHost[host] = seenByHost[host] ?? []
        requestedByHost[host] = nil
        if host == activeHost {
            hostStatus.hide()
        }
        // The handshake goes to this host, so point the send helpers at it for a moment.
        let previous = activeHost
        activeHost = host
        sendHello()
        sendAppearance()
        activeHost = previous
        refreshSidebar()
    }

    /// One host dropped: retry *that* host on its own budget, without touching the others.
    private func hostLost(host: String, _ reason: String) {
        clients[host] = nil
        if host == "local", stoppedByUser {
            stoppedByUser = false
            if host == activeHost {
                showStopped("Session host stopped. Retry starts a new one.")
            }
            refreshSidebar()
            return
        }
        if host == activeHost, let refused = refusedReason {
            refusedReason = nil
            showStopped(refused)
            return
        }
        recovering[host] = 0
        recover(host: host, reason: reason)
    }

    private func recover(host: String, reason: String) {
        let remote = host != "local"
        let attempt = recovering[host] ?? 0
        let allowed = remote
            ? HostRecovery.canRetryRemote(after: attempt)
            : HostRecovery.canRetry(after: attempt)
        guard allowed else {
            if host == activeHost { showStopped(reason) }
            refreshSidebar()
            return
        }
        recovering[host] = attempt + 1
        if host == activeHost {
            show(hostStatus: .reconnecting(attempt: attempt + 1))
        }
        let delay = remote ? HostRecovery.remoteRetryDelay : HostRecovery.retryDelay
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self else { return }
            if remote {
                self.reconnectRemote(host: host, reason: reason)
                return
            }
            guard !self.connectHost() else { return }
            // No socket at all: have launchd start the host and give it a moment to bind.
            self.startLocalHost()
            DispatchQueue.main.asyncAfter(deadline: .now() + HostRecovery.retryDelay) { [weak self] in
                guard let self, !self.connectHost() else { return }
                self.recover(host: host, reason: reason)
            }
        }
    }

    /// A dropped ssh channel comes back through the same A→B→C probe. The remote host kept
    /// the panes, which is what makes a 30 s drop survivable at all.
    private func reconnectRemote(host: String, reason: String) {
        guard let session = remotes[host] else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            do {
                // Never silently fall back to plain ssh here: that would replace the
                // persistent session with one that dies with the window.
                let outcome = try session.connect(allowDegraded: false)
                DispatchQueue.main.async { [weak self] in self?.apply(outcome, host: host) }
            } catch {
                DispatchQueue.main.async { [weak self] in
                    self?.recover(host: host, reason: "\(reason) (\(error))")
                }
            }
        }
    }

    /// The host runs as a launchd agent so it outlives this process (issue #2); a plain
    /// child is the fallback only when launchd refuses, and then says so in the log.
    private func startLocalHost() {
        guard let path = HostLauncher.find(candidates: HostLauncher.candidates()) else {
            debugLog("no pigeon-host binary found")
            return
        }
        let environment = ProcessInfo.processInfo.environment
        if let error = hostAgent.start(
            hostPath: path, socket: Self.defaultSocketPath(), shell: environment["SHELL"])
        {
            debugLog("launchd: \(error); starting the host as a child instead")
            HostLauncher.launch(path: path)
        }
    }

    private func showStopped(_ reason: String) {
        if activeHost != "local" {
            show(hostStatus: .stopped(
                detail: "\(reason)\nCheck `ssh \(activeHost)`, then Retry."))
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
        recovering[activeHost] = 0
        if activeHost != "local" {
            disconnect(host: activeHost)
            startRemote(activeHost, select: true)
            return
        }
        if !connectHost() {
            recover(host: "local", reason: "No session host is running.")
        }
    }

    /// Hangs up one host without disturbing the others.
    private func disconnect(host: String) {
        clients[host]?.close()
        clients[host] = nil
        remotes[host]?.close()
        remotes[host] = nil
        rowsByHost[host] = nil
        focusedByHost[host] = nil
        seenByHost[host] = nil
        requestedByHost[host] = nil
        pendingByHost[host] = nil
        seqByHost[host] = nil
        staleByHost[host] = nil
        instanceByHost[host] = nil
    }

    private static func defaultSocketPath() -> String {
        if let runtime = ProcessInfo.processInfo.environment["XDG_RUNTIME_DIR"] {
            return runtime + "/pigeon.sock"
        }
        return "/tmp/pigeon-\(getuid()).sock"
    }

    // MARK: - host keys and sidebar ids

    /// `"local"` plus every known remote, in a stable order.
    private func hostKeys() -> [String] {
        hostOrder = ["local"] + hosts.hosts.filter { $0 != "local" }
        return hostOrder
    }

    private func hostKey(_ host: String?) -> String { host ?? "local" }

    /// Sessions of different hosts both start at 1, so the sidebar id carries the host.
    private func rowId(host: String, session: Int) -> Int {
        let keys = hostKeys()
        let index = keys.firstIndex(of: host).map { $0 + 1 } ?? keys.count + 1
        return index * 100_000 + session
    }

    private func splitRowId(_ id: Int) -> (host: String, session: Int)? {
        guard id > 0 else { return nil }
        let keys = hostKeys()
        let index = id / 100_000 - 1
        guard keys.indices.contains(index) else { return nil }
        return (keys[index], id % 100_000)
    }

    /// Unread counts are persisted per host; local keeps the old key so existing state works.
    private func unreadKey(host: String, session: Int) -> String {
        host == "local" ? String(session) : "\(host):\(session)"
    }

    private func requestID() -> UInt32 {
        defer { nextRequest &+= 1 }
        return nextRequest
    }

    private func send(_ build: (OpaquePointer) -> Void, type: PgMsgType) {
        guard let client, let message = Wire.make(type) else { return }
        sentCounts[type, default: 0] += 1
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
            // `delta` sends only the rows that changed: a keystroke costs ~2 KB instead of a
            // full screen (~100 KB on a large pane), which is what makes ssh feel slow.
            Wire.set(message, strings: "features", ["scrollback", "delta"])
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
        let stale = staleByHost[activeHost]?.contains(pane) ?? false
        if !force, (haveSnapshot.contains(pane) && !stale) || !requested.insert(pane).inserted {
            return
        }
        staleByHost[activeHost]?.remove(pane)
        let since = HostRecovery.resumeSeq(
            drawn: frameSeq[pane], stale: stale, onScreen: haveSnapshot.contains(pane))
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", pane)
            if let since { Wire.set(message, u32: "since", since) }
            // Scrollback is read through `scroll`, so the history (up to 2 MB) never needs to
            // ride along — every session click re-attaches.
            Wire.set(message, bool: "history", false)
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
        lastSelectedId = id
        guard let (host, session) = splitRowId(id) else { return }
        if host != activeHost {
            activate(host: host)
        }
        activeHost = host
        guard let row = rowsByHost[host]?[session] else { return }
        debugLog("select host=\(host) id=\(session) activePane=\(row.activePane) panes=\(row.panes)")
        selectedSession = session
        focusedPane = row.activePane
        // Clicking a session must hand the keyboard back to its terminal: the click left
        // focus on the sidebar table, and nothing else would move it back when the layout
        // does not change.
        container.focusPane(row.activePane)
        for pane in row.panes { sendAttach(pane, force: true) }
        send({ message in Wire.set(message, u32: "pane_id", row.activePane) }, type: PG_MSG_FOCUS)
        if var current = rows[session] {
            current.unread = 0
            rows[session] = current
            state.unread[unreadKey(host: host, session: session)] = 0
            store.save(state)
            sendAck(current.activePane, current.seq)
        }
        refreshSidebar()
    }

    private func closeSession(_ id: Int) {
        guard let (host, session) = splitRowId(id) else { return }
        closeSession(host: host, session: session)
    }

    /// Closing a session on a specific host (the pane-exited path knows which one).
    private func closeSession(host: String, session: Int) {
        let previous = activeHost
        activeHost = host
        defer { activeHost = previous }
        let id = session
        guard let row = rows[id] else { return }
        for pane in row.panes { sendClosePane(pane) }
        rows.removeValue(forKey: id)
        state.unread.removeValue(forKey: unreadKey(host: host, session: session))
        store.save(state)
        if selectedSession == id { selectedSession = nil }
        refreshSidebar()
        requestSessions()
        quitIfNoSessionsLeft()
    }

    /// Closing the last session (from the sidebar, ⌘⇧W, or its shell exiting) quits the app,
    /// like closing a terminal's last tab. Only a close leads here: a host that drops its
    /// connection, or one that has no sessions yet at launch, never quits pigeon.
    private func quitIfNoSessionsLeft() {
        let remaining = hostKeys().filter { clients[$0] != nil }
            .reduce(0) { $0 + (rowsByHost[$1]?.count ?? 0) }
        guard SidebarModel.shouldQuitAfterClose(
            remainingSessions: remaining, creatingSession: pendingSelectSession != nil)
        else { return }
        debugLog("last session closed: quitting")
        // Next turn, so the `close_pane` frames written above are on their way first.
        DispatchQueue.main.async { NSApp.terminate(nil) }
    }

    private func markRead(_ id: Int) {
        guard let (host, session) = splitRowId(id) else { return }
        activeHost = host
        guard var row = rows[session] else { return }
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

    @objc private func stopLocalHost(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = "Stop the session host?"
        alert.informativeText =
            "Every session on this Mac ends, including programs still running in them."
        alert.addButton(withTitle: "Stop Host")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        if let error = hostAgent.stop() {
            // Most likely a host this launchd job did not start (run by hand, or by an
            // older build); it keeps running, so say so instead of pretending.
            let failure = NSAlert()
            failure.messageText = "The session host is still running"
            failure.informativeText = error
            failure.runModal()
            return
        }
        // The socket drop would normally restart the host; this time it must stay down.
        stoppedByUser = true
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

    /// Shows `host`'s terminal, connecting it if it is not up yet.
    private func activate(host: String) {
        debugLog("activate host=\(host) from=\(activeHost)")
        if host != activeHost {
            activeHost = host
            selectedSession = nil
            localSession = nil
            container.apply(.empty)
            // A degraded session pointed these at its local VT; the protocol path takes over.
            container.onPaneInput = { [weak self] pane, bytes in self?.sendKeys(pane, bytes) }
            container.onPaneResize = { [weak self] pane, cols, rows in
                self?.sendResize(pane, cols: cols, rows: rows)
            }
        }
        if clients[host] == nil {
            if host == "local" {
                if !connectHost() {
                    hostLost(host: "local", "No session host is running.")
                }
            } else {
                startRemote(host, select: true)
            }
        }
        if selectedSession == nil, let first = (rowsByHost[host] ?? [:]).keys.sorted().first {
            select(rowId(host: host, session: first))
        }
        refreshSidebar()
    }

    @objc private func nextNeedsYou(_ sender: Any?) {
        let sessions = sidebarSessions()
        let current = selectedSession.map { rowId(host: activeHost, session: $0) }
        guard let next = SidebarModel.nextNeedsYou(sessions, after: current) else { return }
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
        var sessions: [SidebarSession] = []
        // Sessions of every *connected* host, grouped under `host / project`. An offline host
        // simply disappears from the sidebar; its sessions come back when it reconnects.
        for key in hostKeys() where clients[key] != nil {
            let hostRows = (rowsByHost[key] ?? [:]).values.sorted { $0.id < $1.id }
            for row in hostRows {
                sessions.append(
                    SidebarSession(
                        id: rowId(host: key, session: row.id),
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
                        host: key
                    ))
            }
        }
        return sessions
    }

    /// The row's name: the program the host picked (`vim`, `zsh`), never a shell prompt —
    /// the machine and directory are already on the row's group heading.
    private func appName(info: PgSessionInfo, previous: Row?) -> String {
        guard info.active_pane != 0 else { return previous?.app ?? "shell" }
        let program = Wire.string(info.active_program)
        if !program.isEmpty { return program }
        return Wire.string(info.active_title)
    }

    /// "Jeffrey's MacBook Air"; looked up once because the lookup can touch the resolver.
    private lazy var localMachineName: String = Host.current().localizedName ?? ""

    private func refreshSidebar() {
        let raw = SidebarModel.sections(sidebarSessions(), order: state.order)
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
                .map { "\($0):\(rowsByHost[$0]?.count ?? -1)" }
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

    /// Routes one host's message through the handlers, which read and write "the active
    /// host" state; the window itself only follows the visible host.
    private func handle(_ message: OpaquePointer, host: String) {
        let visible = host == activeHost
        let previous = activeHost
        activeHost = host
        defer { activeHost = previous }
        handleActive(message, visible: visible)
    }

    private func handleActive(_ message: OpaquePointer, visible: Bool) {
        switch Wire.type(message) {
        case PG_MSG_HELLO_ACK:
            let instance = Wire.string(message, "instance")
            if instanceByHost[activeHost] != instance {
                // A restarted host numbers its frames from scratch.
                staleByHost[activeHost] = nil
                seqByHost[activeHost] = nil
            }
            instanceByHost[activeHost] = instance
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
            flushPendingSnapshots()
            for pane in model.panes.map(\.id) { sendAttach(pane) }
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
            // `close_pane`, so dropping the row locally is not enough — closing the session is
            // what makes the host forget it, otherwise the next `sessions` refresh re-adds it.
            let pane = Wire.u32(message, "pane_id")
            guard let id = rows.first(where: { $0.value.panes.contains(pane) })?.key else { break }
            debugLog("pane exited pane=\(pane) -> closing session \(id)")
            // `id` is the host's own session number; `closeSession(_:)` wants a sidebar id.
            closeSession(host: activeHost, session: id)
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
                app: appName(info: info, previous: previous),
                title: "session \(id)",
                status: previous?.status ?? .idle,
                seq: previous?.seq ?? 0,
                unread: previous?.unread ?? state.unread[unreadKey(host: activeHost, session: id)] ?? 0,
                lastSeen: previous?.lastSeen ?? Date(),
                sizeSource: previous?.sizeSource ?? "",
                clients: previous?.clients ?? 1,
                host: activeHost,
                group: Grouping.pinnedGroup(
                    existing: previous?.group,
                    cwd: Wire.string(info.cwd),
                    gitRoot: GitRoot.find(cwd: Wire.string(info.cwd)))
            )
        }
        rows = updated
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            let apps = rows.values.map { "\($0.id):\($0.app)" }.sorted().joined(separator: " ")
            FileHandle.standardError.write(Data("pigeon: sessions apps=\(apps)\n".utf8))
        }
        if rows.isEmpty, !createdInitialSessionFor.contains(activeHost) {
            // An empty host gets one session instead of an empty group.
            createdInitialSessionFor.insert(activeHost)
            debugLog("no sessions on \(activeHost): creating one")
            sendNewSession(cwd: nil)
            return
        }
        // A background host keeps its sessions and status; it must not steal the window.
        guard visible else {
            for row in rows.values { sendAttach(row.activePane) }
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

    /// Exercises the three input paths a window has — menu key equivalents, first-responder
    /// keys, and a sidebar row selection — through AppKit itself. Run by
    /// `tools/m2-gui-check.sh`: this session cannot activate windows, and these paths are
    /// dispatched by the active app, so driving them in-process is the only honest check.
    private func beginSelfTestIfNeeded() {
        guard selfTest, !selfTestRunning, !rows.isEmpty else { return }
        selfTestRunning = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            self?.selfTestStep(0)
        }
    }

    private func keyEvent(
        _ characters: String, code: UInt16, flags: NSEvent.ModifierFlags
    ) -> NSEvent? {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: flags,
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window?.windowNumber ?? 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: false,
            keyCode: code)
    }

    private func selfTestStep(_ step: Int) {
        func report(_ ok: Bool, _ what: String) {
            FileHandle.standardError.write(
                Data("SELFTEST \(ok ? "ok" : "FAIL") step=\(step) \(what)\n".utf8))
            if ok {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
                    self?.selfTestStep(step + 1)
                }
            } else {
                FileHandle.standardError.write(Data("SELFTEST FAIL\n".utf8))
                exit(1)
            }
        }
        func waitFor(_ type: PgMsgType, _ what: String) {
            let deadline = Date().addingTimeInterval(5)
            func poll() {
                if (sentCounts[type] ?? 0) > 0 {
                    report(true, what)
                } else if Date() < deadline {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { poll() }
                } else {
                    report(false, what)
                }
            }
            poll()
        }
        func menuItem(
            _ characters: String, _ flags: NSEvent.ModifierFlags
        ) -> NSMenuItem? {
            func find(_ menu: NSMenu?) -> NSMenuItem? {
                for item in menu?.items ?? [] {
                    if item.keyEquivalent == characters,
                       item.keyEquivalentModifierMask == flags { return item }
                    if let found = find(item.submenu) { return found }
                }
                return nil
            }
            return find(NSApp.mainMenu)
        }

        func pressMenu(
            _ characters: String,
            _ code: UInt16,
            _ flags: NSEvent.ModifierFlags,
            expects type: PgMsgType,
            _ what: String
        ) {
            guard let event = keyEvent(characters, code: code, flags: flags) else {
                report(false, "\(what) (cannot build a key event)")
                return
            }
            // The key window is what makes the responder chain reachable; this session cannot
            // activate the app, so if the key equivalent does not dispatch we deliver the very
            // action the menu item names — the rest of the path is unchanged.
            window.makeKey()
            window.makeFirstResponder(container)
            let took = NSApp.mainMenu?.performKeyEquivalent(with: event) ?? false
            if (sentCounts[type] ?? 0) == 0, let item = menuItem(characters, flags),
               let action = item.action
            {
                NSApp.sendAction(action, to: item.target ?? container, from: item)
            }
            waitFor(type, took ? what : "\(what) (menu action, key equivalent not dispatched)")
        }

        switch step {
        case 0:
            pressMenu("d", 2, [.command], expects: PG_MSG_SPLIT_PANE, "split")
        case 1:
            pressMenu("\r", 36, [.command, .shift], expects: PG_MSG_ZOOM_PANE, "zoom")
        case 2:
            // The zoom answer has to be back before the same key means "unzoom".
            guard container.model.zoom != 0 else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
                    self?.selfTestStep(2)
                }
                return
            }
            pressMenu("\r", 36, [.command, .shift], expects: PG_MSG_UNZOOM, "unzoom")
        case 3:
            let pageUp = UnicodeScalar(0xF72C).map { String(Character($0)) } ?? ""
            guard let pane = focusedPane,
                  let view = container.paneView(pane),
                  let event = keyEvent(pageUp, code: 116, flags: [])
            else {
                report(false, "scroll (no pane to send PageUp to)")
                return
            }
            view.keyDown(with: event)
            waitFor(PG_MSG_SCROLL, "scroll")
        case 4:
            guard let pane = focusedPane, let view = container.paneView(pane) else {
                report(false, "typing (no pane)")
                return
            }
            guard window.firstResponder === view else {
                report(false, "typing (the focused pane is not first responder)")
                return
            }
            guard let event = keyEvent("a", code: 0, flags: []) else {
                report(false, "typing (cannot build a key event)")
                return
            }
            view.keyDown(with: event)
            waitFor(PG_MSG_KEYS, "typing")
        case 5:
            // Click a session row the way a user does, then type again: this is the
            // "I clicked a session and the terminal stopped listening" case.
            guard let session = rows.keys.min(),
                  sidebar.selectRow(id: rowId(host: activeHost, session: session))
            else {
                report(false, "session-click (no session row to select)")
                return
            }
            guard let pane = focusedPane, let view = container.paneView(pane),
                  window.firstResponder === view
            else {
                report(false, "session-click (the terminal did not get the keyboard back)")
                return
            }
            let before = sentCounts[PG_MSG_KEYS] ?? 0
            if let event = keyEvent("b", code: 0, flags: []) {
                view.keyDown(with: event)
            }
            if (sentCounts[PG_MSG_KEYS] ?? 0) > before {
                report(true, "session-click then typing")
            } else {
                report(false, "session-click then typing (no keys sent)")
            }
        default:
            FileHandle.standardError.write(Data("SELFTEST PASS\n".utf8))
            exit(0)
        }
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
                frameSeq[pane] = Wire.u32(decoded, "seq")
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
