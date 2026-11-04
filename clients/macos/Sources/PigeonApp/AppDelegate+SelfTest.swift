import AppKit
import PigeonCore

extension AppDelegate {
    /// Exercises the three input paths a window has — menu key equivalents, first-responder
    /// keys, and a sidebar row selection — through AppKit itself. Run by
    /// `tools/checks/m2-gui-check.sh`: this session cannot activate windows, and these paths are
    /// dispatched by the active app, so driving them in-process is the only honest check.
    func beginSelfTestIfNeeded() {
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
        case 6:
            // Edit menu: Select All then Copy must put the screen's text on the pasteboard,
            // and Paste must reach the host as one `paste` message. Driven through the menu
            // items' own actions and the responder chain, like a click on the menu.
            guard let pane = focusedPane, let view = container.paneView(pane) else {
                report(false, "copy (no pane)")
                return
            }
            let saved = NSPasteboard.general.string(forType: .string)
            func run(_ title: String) -> Bool {
                // Menu-bar items carry no title of their own; the submenu does.
                let edit = NSApp.mainMenu?.items.first { $0.submenu?.title == "Edit" }?.submenu
                guard let item = edit?.item(withTitle: title),
                      let action = item.action
                else { return false }
                // This session cannot make the window key, so `sendAction(to: nil)` finds
                // no chain; walk this window's chain from its first responder instead, which
                // is the chain the menu uses once the window is key.
                return window.firstResponder?.tryToPerform(action, with: item) ?? false
            }
            window.makeFirstResponder(view)
            let selected = run("Select All")
            let copied = run("Copy")
            let text = NSPasteboard.general.string(forType: .string) ?? ""
            let visible = view.selectedText
            guard selected, copied, !visible.isEmpty, text == visible else {
                report(false, "copy (selectAll=\(selected) copy=\(copied) pasteboard=\(text.count) chars)")
                return
            }
            let before = sentCounts[PG_MSG_PASTE] ?? 0
            let pasted = run("Paste")
            if let saved {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(saved, forType: .string)
            }
            report(pasted && (sentCounts[PG_MSG_PASTE] ?? 0) > before, "select all, copy, paste")
        default:
            FileHandle.standardError.write(Data("SELFTEST PASS\n".utf8))
            exit(0)
        }
    }

}
