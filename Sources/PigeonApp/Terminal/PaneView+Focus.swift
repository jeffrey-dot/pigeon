import AppKit

/// Keyboard focus: the cursor is drawn hollow when the pane or its window loses the
/// keyboard, and half-composed input is committed rather than stranded.
extension PaneView {
    /// Whether keys typed now land here: first responder of the key window.
    var hasKeyboardFocus: Bool {
        guard let window else { return false }
        return window.isKeyWindow && window.firstResponder === self
    }

    override func becomeFirstResponder() -> Bool {
        defer { screenChanged() }
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        // Leaving with half-typed pinyin would strand it; commit what the user sees.
        if hasMarkedText() { unmarkText() }
        inputContext?.discardMarkedText()
        defer { screenChanged() }
        return super.resignFirstResponder()
    }

    override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        let center = NotificationCenter.default
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification] {
            center.removeObserver(self, name: name, object: window)
            if let newWindow {
                center.addObserver(
                    self, selector: #selector(keyWindowChanged(_:)), name: name, object: newWindow)
            }
        }
    }

    /// The cursor goes hollow with the window, not only with the pane.
    @objc func keyWindowChanged(_ note: Notification) {
        screenChanged()
    }
}
