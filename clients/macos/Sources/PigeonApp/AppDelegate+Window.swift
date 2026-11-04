import AppKit
import PigeonCore

// Window, title bar and menu: the chrome built once at launch.

extension AppDelegate {
    func buildWindow() {
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
        // `docs/ui.md` §2: the sidebar runs under a transparent titlebar, with the traffic
        // lights sitting inside its 56pt header (harp: TL:64, VT:9112).
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.isMovableByWindowBackground = true
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didChangeOcclusionStateNotification] {
            NotificationCenter.default.addObserver(
                self, selector: #selector(statusWindowBecameVisible(_:)), name: name, object: window)
        }
        NotificationCenter.default.addObserver(
            self, selector: #selector(statusWindowBecameVisible(_:)),
            name: NSApplication.didBecomeActiveNotification, object: NSApp)
        window.isOpaque = false
        window.backgroundColor = .clear
        window.contentMinSize = NSSize(width: 640, height: 400)

        sidebar = SidebarViewController()
        sidebar.onSelect = { [weak self] id in self?.select(id) }
        sidebar.onClose = { [weak self] id in self?.closeSession(id) }
        sidebar.onShowStatusRecords = { [weak self] id in self?.showStatusRecords(for: id) }
        sidebar.onMarkRead = { [weak self] id in self?.markRead(id) }
        sidebar.onRename = { [weak self] id, name in self?.rename(id, name) }
        sidebar.onRenameGroup = { [weak self] group, name in self?.renameGroup(group, name) }
        sidebar.onReorder = { [weak self] order in self?.reorder(order) }
        sidebar.onNewSession = { [weak self] cwd in self?.newSession(cwd.isEmpty ? nil : cwd) }
        sidebar.newSessionMenu = { [weak self] in self?.buildNewSessionMenu() ?? NSMenu() }
        sidebar.onToggleSidebar = { [weak self] in self?.toggleSidebar(nil) }

        container = SplitContainerView(frame: frame)
        container.optionAsMeta = settings.optionAsMeta
        container.onSplit = { [weak self] axis in self?.split(axis) }
        container.onClose = { [weak self] in self?.closeFocusedPane() }
        container.onFocus = { [weak self] pane in self?.focus(pane) }
        container.onResizeSplit = { [weak self] split, ratio in self?.resizeSplit(split, ratio) }
        container.onPaneInput = { [weak self] pane, bytes in self?.sendKeys(pane, bytes) }
        container.onPanePaste = { [weak self] pane, text in self?.sendPaste(pane, text) }
        container.onPaneResize = { [weak self] pane, cols, rows in
            self?.sendResize(pane, cols: cols, rows: rows)
        }
        // Scrollback lives in the host; the window only says which way to look.
        container.onPaneScroll = { [weak self] pane, lines in self?.sendScroll(pane, lines: lines) }
        container.onPaneScrollBottom = { [weak self] pane in self?.sendScroll(pane, lines: 0) }
        container.onToggleZoom = { [weak self] in self?.toggleZoom() }

        splitView = SidebarSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin
        // `docs/ui.md` §2: harp's 272pt by default, user drag overrides (low priority).
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
        alignSidebarHeaderControls()
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data(
                "pigeon: sidebar width=\(Int(sidebar.view.frame.width)) window=\(Int(window.frame.width))\n".utf8
            ))
        }
        refreshSidebar()
    }

    /// The native titlebar owns hit testing above contentLayoutRect on macOS 15.
    /// Keep our buttons inside the unobstructed content, even with fullSizeContentView.
    private func alignSidebarHeaderControls() {
        guard let zoom = window.standardWindowButton(.zoomButton) else { return }
        let center = zoom.convert(NSPoint(x: zoom.bounds.midX, y: zoom.bounds.midY), to: nil)
        let topInset = max(0, window.frame.height - window.contentLayoutRect.maxY)
        sidebar.alignHeaderControls(centerFromTop: max(
            window.frame.height - center.y, topInset + SidebarMetrics.headerButtonSize / 2))
    }

    func buildMenu() {
        let mainMenu = NSMenu()
        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(updates.menuItem)
        appMenu.addItem(.separator())
        // Quitting leaves the host and its sessions running; ending them is its own command.
        let stopHost = NSMenuItem(
            title: "End All Sessions on This Mac…", action: #selector(stopLocalHost(_:)), keyEquivalent: "")
        stopHost.target = self
        appMenu.addItem(stopHost)
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit pigeon", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu

        // Standard actions with no target: AppKit routes them to the first responder, which
        // is the focused pane (or the sidebar's rename field while editing a name).
        let editItem = NSMenuItem()
        mainMenu.addItem(editItem)
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = editMenu

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
        viewMenu.addItem(.separator())
        let meta = NSMenuItem(
            title: "Use Option as Meta Key", action: #selector(toggleOptionAsMeta(_:)), keyEquivalent: "")
        meta.target = self
        meta.state = settings.optionAsMeta ? .on : .off
        viewMenu.addItem(meta)
        viewItem.submenu = viewMenu

        let sessionItem = NSMenuItem()
        mainMenu.addItem(sessionItem)
        let sessionMenu = NSMenu(title: "Session")
        sessionMenu.addItem(withTitle: "Status Records…", action: #selector(showStatusRecords(_:)), keyEquivalent: "")
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
}
