import AppKit
import PigeonCore

/// Wires the window to a `pigeon-host` and drives the single pane.
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow!
    private var pane: PaneView!
    private var client: HostClient?
    private var paneID: UInt32?

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
        pane = PaneView(frame: frame)
        pane.onInput = { [weak self] bytes in self?.sendKeys(bytes) }
        pane.onResize = { [weak self] cols, rows in self?.sendResize(cols: cols, rows: rows) }
        window.contentView = pane
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(pane)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func buildMenu() {
        let mainMenu = NSMenu()
        let appMenu = NSMenuItem()
        mainMenu.addItem(appMenu)
        let submenu = NSMenu()
        submenu.addItem(
            withTitle: "Quit pigeon",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        appMenu.submenu = submenu
        NSApp.mainMenu = mainMenu
    }

    // MARK: - host

    private func connect() {
        let path = Self.defaultSocketPath()
        do {
            let client = try HostClient.connect(path: path)
            client.onMessage = { [weak self] message in
                // AppKit is main-thread only; the socket runs on its own queue.
                DispatchQueue.main.async {
                    defer { Wire.free(message) }
                    self?.handle(message)
                }
            }
            client.onClose = { [weak self] in
                DispatchQueue.main.async {
                    NSLog("pigeon: host disconnected")
                    self?.client = nil
                }
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

    private func sendHello() {
        guard let client, let hello = Wire.make(PG_MSG_HELLO) else { return }
        defer { Wire.free(hello) }
        Wire.set(hello, u32: "proto", 0x0100)
        Wire.set(hello, str: "client", "pigeon-mac")
        Wire.set(hello, strings: "features", ["scrollback"])
        client.send(hello)
    }

    private func sendNewSession() {
        guard let client, let message = Wire.make(PG_MSG_NEW_SESSION) else { return }
        defer { Wire.free(message) }
        Wire.set(message, u32: "id", 1)
        client.send(message)
    }

    private func sendAttach(_ paneID: UInt32) {
        guard let client, let message = Wire.make(PG_MSG_ATTACH) else { return }
        defer { Wire.free(message) }
        Wire.set(message, u32: "id", 2)
        Wire.set(message, u32: "pane_id", paneID)
        client.send(message)
        pane.forceSyncGrid()
    }

    private func sendKeys(_ bytes: [UInt8]) {
        guard let client, let paneID, let message = Wire.make(PG_MSG_KEYS) else { return }
        defer { Wire.free(message) }
        Wire.set(message, u32: "pane_id", paneID)
        Wire.set(message, bytes: "bytes", bytes)
        client.send(message)
    }

    private func sendResize(cols: UInt16, rows: UInt16) {
        guard let client, let paneID, let message = Wire.make(PG_MSG_RESIZE_PANE) else { return }
        defer { Wire.free(message) }
        Wire.set(message, u32: "pane_id", paneID)
        Wire.set(message, u32: "cols", UInt32(cols))
        Wire.set(message, u32: "rows", UInt32(rows))
        client.send(message)
    }

    private func handle(_ message: OpaquePointer) {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: <- \(Wire.type(message))\n".utf8))
        }
        switch Wire.type(message) {
        case PG_MSG_HELLO_ACK:
            // The host pushes `sessions` right after the ack.
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
                    paneID = panes[0]
                    sendAttach(panes[0])
                }
            }
        case PG_MSG_CREATED:
            paneID = Wire.u32(message, "pane_id")
            if let paneID { sendAttach(paneID) }
        case PG_MSG_SNAPSHOT:
            guard let snapshot = pg_msg_snapshot(message) else { break }
            defer { pg_snapshot_free(snapshot) }
            pane.apply(snapshot: snapshot)
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
