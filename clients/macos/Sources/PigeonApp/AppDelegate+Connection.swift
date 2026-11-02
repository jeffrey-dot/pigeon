import AppKit
import PigeonCore

// Reaching the active host and keeping that link alive: remote a/b/c, heartbeat, loss,
// recovery, and the local host that gets started on demand.

extension AppDelegate {
    /// Connects to one machine (`docs/remote.md` §4: every known host gets its own ssh
    /// connection; the window only shows the one whose session is selected).
    func startRemote(_ host: String, select: Bool = false) {
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
                    guard let self else { return }
                    self.connectingHost = nil
                    self.refreshSidebar()
                    self.debugLog("cannot reach \(host): \(error)")
                    if host == self.activeHost {
                        self.show(hostStatus: .stopped(
                            detail: HostRecovery.remoteHelp(host: host, reason: "\(error)")))
                    }
                }
            }
        }
    }

    func apply(_ outcome: RemoteSession.Outcome, host: String) {
        debugLog("remote transport=\(outcome.transport.rawValue) host=\(host) \(outcome.detail)")
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
        container.onPanePaste = { [weak session] _, text in
            session?.write(Array(text.replacingOccurrences(of: "\n", with: "\r").utf8))
        }
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
    func connectHost() -> Bool {
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
                self?.hostLost(host: host, "The connection was closed.")
            }
        }
        clients[host] = client
        startHeartbeat()
        recovering[host] = 0
        // A reconnect keeps the screens on display; every pane drawn so far re-attaches,
        // asking only for what it missed.
        connections[host, default: HostConnection()].reconnected(now: Date())
        if host == activeHost {
            hostStatus.hide()
        }
        // The handshake goes to this host, so point the send helpers at it for a moment.
        onHost(host) {
            sendHello()
            sendAppearance()
        }
        refreshSidebar()
    }

    /// A link that went silent gets a `ping`; one silent past the timeout is closed, which
    /// runs the normal reconnect path (`onClose` → `hostLost`).
    private func startHeartbeat() {
        guard heartbeatTimer == nil else { return }
        let timer = Timer(timeInterval: 5, repeats: true) { [weak self] _ in
            self?.tickHeartbeats()
        }
        timer.tolerance = 1
        RunLoop.main.add(timer, forMode: .common)
        heartbeatTimer = timer
    }

    private func tickHeartbeats() {
        let now = Date()
        for host in Array(connections.keys) {
            guard var beat = connections[host]?.heartbeat, let client = clients[host] else { continue }
            let action = beat.tick(at: now)
            connections[host]?.heartbeat = beat
            switch action {
            case .none:
                break
            case .ping:
                guard let ping = Wire.make(PG_MSG_PING) else { break }
                // Only echoed back; CBOR integers widen, so a u32 decodes as the host's u64.
                Wire.set(ping, u32: "ts", UInt32(truncatingIfNeeded: Int(now.timeIntervalSince1970)))
                client.send(ping)
                Wire.free(ping)
            case .dead:
                debugLog("host \(host) silent for \(Int(Heartbeat.timeout))s: reconnecting")
                connections[host]?.heartbeat = nil
                client.close()
                hostLost(host: host, "The connection stopped responding.")
            }
        }
    }

    /// One host dropped: retry *that* host on its own budget, without touching the others.
    func hostLost(host: String, _ reason: String) {
        clients[host] = nil
        connections[host]?.heartbeat = nil
        if host == "local", stoppedByUser {
            stoppedByUser = false
            if host == activeHost {
                showStopped("All sessions on this Mac were ended. Retry starts new ones.")
            }
            refreshSidebar()
            return
        }
        if let refused = refusedReason {
            // Retrying a refused handshake only repeats it; say why and stop.
            refusedReason = nil
            if host == activeHost { showStopped(refused) }
            refreshSidebar()
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
                    self?.debugLog("reconnect to \(host) failed: \(error)")
                    self?.recover(host: host, reason: Self.userReason(reason, error))
                }
            }
        }
    }

    /// The host starts like a `tmux` server (`pigeon-server --detach`): its own process group,
    /// nothing registered with the system, so it outlives this window without macOS listing
    /// it as a background item. It lives until "End All Sessions on This Mac…" or logout.
    private func startLocalHost() {
        guard let path = HostLauncher.find(candidates: HostLauncher.candidates()) else {
            debugLog("no pigeon-server binary found")
            return
        }
        let log = TerminalSettingsStore.defaultURL.deletingLastPathComponent()
            .appendingPathComponent("host.log").path
        let arguments = HostLauncher.detachArguments(
            socket: Self.defaultSocketPath(),
            shell: ProcessInfo.processInfo.environment["SHELL"],
            log: log)
        if !HostLauncher.launch(path: path, arguments: arguments) {
            debugLog("cannot start \(path)")
        }
    }

    private func showStopped(_ reason: String) {
        if activeHost != "local" {
            show(hostStatus: .stopped(detail: HostRecovery.remoteHelp(host: activeHost, reason: reason)))
            return
        }
        show(hostStatus: .stopped(detail: "\(reason)\nClick Retry to start it again."))
    }

    /// "Can't connect" is worth showing as is; any other probe failure is plumbing the user
    /// cannot act on, so they get the plain reason and the details go to the log.
    private static func userReason(_ reason: String, _ error: Error) -> String {
        if case HostInstaller.Failure.unreachable = error { return "\(error)" }
        return reason
    }

    private func show(hostStatus state: HostRecovery.State) {
        guard let text = HostRecovery.banner(for: state) else {
            hostStatus.hide()
            return
        }
        hostStatus.show(message: text, action: HostRecovery.action(for: state))
    }

    func retryHost() {
        recovering[activeHost] = 0
        if activeHost != "local" {
            disconnect(host: activeHost)
            startRemote(activeHost, select: true)
            return
        }
        if !connectHost() {
            recover(host: "local", reason: "Pigeon's background service isn't running.")
        }
    }

    /// Hangs up one host without disturbing the others.
    private func disconnect(host: String) {
        clients[host]?.close()
        clients[host] = nil
        remotes[host]?.close()
        remotes[host] = nil
        // The selection survives a hang-up, as it did when it lived in its own table.
        let selected = connections[host]?.selectedSession
        connections[host] = nil
        if let selected { connections[host] = HostConnection(selectedSession: selected) }
    }

    private static func defaultSocketPath() -> String {
        if let runtime = ProcessInfo.processInfo.environment["XDG_RUNTIME_DIR"] {
            return runtime + "/pigeon.sock"
        }
        return "/tmp/pigeon-\(getuid()).sock"
    }
}
