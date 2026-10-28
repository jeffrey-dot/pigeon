import AppKit
import PigeonCore

/// Wires the window to a `pigeon-host`: one split container, one pane view per
/// pane, and the protocol messages that keep them in sync.
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow!
    private var container: SplitContainerView!
    private var client: HostClient?
    private var sessionID: UInt32?
    private var focusedPane: UInt32?
    /// Panes this window has already asked for a snapshot of.
    private var attached: Set<UInt32> = []
    private var nextRequest: UInt32 = 1

    func applicationDidFinishLaunching(_ notification: Notification) {
        buildMenu()
        buildWindow()
        connect()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }

    // MARK: - window

    private func buildWindow() {
        let frame = NSRect(x: 0, y: 0, width: 900, height: 560)
        window = NSWindow(
            contentRect: frame,
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "pigeon"
        container = SplitContainerView(frame: frame)
        container.onSplit = { [weak self] axis in self?.split(axis) }
        container.onClose = { [weak self] in self?.closeFocused() }
        container.onFocus = { [weak self] pane in self?.focus(pane) }
        container.onResizeSplit = { [weak self] split, ratio in self?.resizeSplit(split, ratio) }
        container.onPaneInput = { [weak self] pane, bytes in self?.sendKeys(pane, bytes) }
        container.onPaneResize = { [weak self] pane, cols, rows in
            self?.sendResize(pane, cols: cols, rows: rows)
        }
        window.contentView = container
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(container)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func buildMenu() {
        let mainMenu = NSMenu()
        let appItem = NSMenuItem()
        mainMenu.addItem(appItem)
        let appMenu = NSMenu()
        appMenu.addItem(
            withTitle: "Quit pigeon",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        appItem.submenu = appMenu

        let viewItem = NSMenuItem()
        mainMenu.addItem(viewItem)
        let viewMenu = NSMenu(title: "View")
        viewMenu.addItem(withTitle: "Split Left/Right", action: #selector(SplitContainerView.splitVertical(_:)), keyEquivalent: "d")
        let down = NSMenuItem(
            title: "Split Top/Bottom",
            action: #selector(SplitContainerView.splitHorizontal(_:)),
            keyEquivalent: "D"
        )
        viewMenu.addItem(down)
        viewMenu.addItem(withTitle: "Close Pane", action: #selector(SplitContainerView.closeFocused(_:)), keyEquivalent: "w")
        viewMenu.addItem(withTitle: "Previous Pane", action: #selector(SplitContainerView.focusPrevious(_:)), keyEquivalent: "[")
        viewMenu.addItem(withTitle: "Next Pane", action: #selector(SplitContainerView.focusNext(_:)), keyEquivalent: "]")
        viewItem.submenu = viewMenu
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
            client.onClose = { [weak self] in
                DispatchQueue.main.async { self?.client = nil }
            }
            self.client = client
            sendHello()
        } catch {
            NSLog("pigeon: cannot reach \(path): \(error)")
            FileHandle.standardError.write(Data("pigeon: cannot reach \(path): \(error)\n".utf8))
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

    private func sendNewSession() {
        send({ message in
            Wire.set(message, u32: "id", requestID())
        }, type: PG_MSG_NEW_SESSION)
    }

    private func sendAttach(_ pane: UInt32) {
        guard attached.insert(pane).inserted else { return }
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

    private func split(_ axis: PgAxis) {
        guard let focusedPane else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", focusedPane)
            Wire.set(message, str: "axis", axis == PG_AXIS_VERTICAL ? "v" : "h")
            Wire.set(message, f32: "ratio", 0.5)
        }, type: PG_MSG_SPLIT_PANE)
    }

    private func closeFocused() {
        guard let focusedPane else { return }
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "pane_id", focusedPane)
        }, type: PG_MSG_CLOSE_PANE)
    }

    private func focus(_ pane: UInt32) {
        focusedPane = pane
        send({ message in
            Wire.set(message, u32: "pane_id", pane)
        }, type: PG_MSG_FOCUS)
    }

    private func resizeSplit(_ split: UInt32, _ ratio: Double) {
        send({ message in
            Wire.set(message, u32: "id", requestID())
            Wire.set(message, u32: "split_id", split)
            Wire.set(message, f32: "ratio", Float(ratio))
        }, type: PG_MSG_RESIZE_SPLIT)
    }

    // MARK: - protocol

    private func handle(_ message: OpaquePointer) {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(
                Data("pigeon: <- \(Wire.type(message))\n".utf8)
            )
        }
        switch Wire.type(message) {
        case PG_MSG_HELLO_ACK:
            break
        case PG_MSG_SESSIONS:
            let count = pg_msg_sessions_count(message)
            if count == 0 {
                sendNewSession()
                break
            }
            var info = PgSessionInfo()
            if pg_msg_session_at(message, 0, &info), info.pane_count > 0 {
                var panes = [UInt32](repeating: 0, count: Int(info.pane_count))
                let written = panes.withUnsafeMutableBufferPointer { buffer in
                    pg_msg_session_panes(message, 0, buffer.baseAddress, buffer.count)
                }
                if written > 0 {
                    sessionID = info.session_id
                    sendAttach(panes[0])
                }
            }
        case PG_MSG_CREATED:
            sessionID = Wire.u32(message, "session_id")
            sendAttach(Wire.u32(message, "pane_id"))
        case PG_MSG_LAYOUT:
            guard let model = LayoutModel(message: message) else { break }
            if model.focus != 0 {
                focusedPane = model.focus
            }
            // Every pane needs its own snapshot; the layout tells us which exist.
            for pane in model.panes.map(\.id) {
                sendAttach(pane)
            }
            container.apply(model)
        case PG_MSG_SNAPSHOT:
            guard let snapshot = pg_msg_snapshot(message) else { break }
            defer { pg_snapshot_free(snapshot) }
            let pane = snapshot.pointee.pane_id
            focusedPane = focusedPane ?? pane
            _ = container.applySnapshot(snapshot)
        case PG_MSG_PANE_EXITED:
            NSLog("pigeon: pane exited with code \(Wire.i32(message, "code"))")
        case PG_MSG_HELLO_ERROR:
            NSLog("pigeon: handshake rejected: \(Wire.string(message, "detail"))")
        case PG_MSG_ERROR:
            NSLog("pigeon: \(Wire.string(message, "code")): \(Wire.string(message, "detail"))")
        default:
            break
        }
    }
}
