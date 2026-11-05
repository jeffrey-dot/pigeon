import AppKit

/// A plain text field keeps search keystrokes out of the terminal's input path.
final class SearchBarView: NSView, NSSearchFieldDelegate {
    let field = NSSearchField()
    private let caseOption = NSButton(checkboxWithTitle: "Match ASCII Case", target: nil, action: nil)
    private let status = NSTextField(labelWithString: "")
    private let previous = NSButton(title: "↑", target: nil, action: nil)
    private let next = NSButton(title: "↓", target: nil, action: nil)
    private let stop = NSButton(title: "Stop", target: nil, action: nil)
    var onChange: ((String, Bool) -> Void)?
    var onNavigate: ((Int) -> Void)?
    var onStop: (() -> Void)?
    var onClose: (() -> Void)?
    var onRestart: (() -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        field.placeholderString = "Find literal text in this pane"
        field.delegate = self
        field.sendsSearchStringImmediately = true
        field.setAccessibilityLabel("Find literal text in this pane")
        caseOption.state = .on
        caseOption.target = self; caseOption.action = #selector(optionsChanged)
        previous.target = self; previous.action = #selector(goPrevious)
        previous.toolTip = "Previous match (Shift-Return)"
        next.target = self; next.action = #selector(goNext)
        next.toolTip = "Next match (Return)"
        stop.target = self; stop.action = #selector(stopSearch)
        let restart = NSButton(title: "Search Again", target: self, action: #selector(restartSearch))
        let close = NSButton(title: "Close", target: self, action: #selector(closeSearch))
        close.toolTip = "Close search and return to live pane (Escape)"
        let top = NSStackView(views: [field, previous, next, close])
        let bottom = NSStackView(views: [caseOption, stop, restart, status])
        let stack = NSStackView(views: [top, bottom])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            top.widthAnchor.constraint(equalTo: stack.widthAnchor),
            bottom.widthAnchor.constraint(equalTo: stack.widthAnchor),
            field.widthAnchor.constraint(greaterThanOrEqualToConstant: 100),
        ])
        status.font = .systemFont(ofSize: 11)
        status.lineBreakMode = .byTruncatingTail
        status.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }
    required init?(coder: NSCoder) { nil }

    override func cancelOperation(_ sender: Any?) { onClose?() }
    var pattern: String { field.stringValue }
    var caseSensitive: Bool { caseOption.state == .on }
    func focus() { window?.makeFirstResponder(field); field.selectText(nil) }
    func update(_ model: TerminalSearch) {
        status.stringValue = model.summary
        status.toolTip = model.summary
        previous.isEnabled = !model.hits.isEmpty
        next.isEnabled = !model.hits.isEmpty
        stop.isEnabled = model.canCancel
    }
    func unavailable() {
        status.stringValue = "This host does not support search with location"
        field.isEnabled = false
        previous.isEnabled = false; next.isEnabled = false; stop.isEnabled = false
    }
    func enable() { field.isEnabled = true }
    func controlTextDidChange(_ notification: Notification) { onChange?(pattern, caseSensitive) }
    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if selector == #selector(NSResponder.cancelOperation(_:)) { onClose?(); return true }
        if selector == #selector(NSResponder.insertNewline(_:)) || selector == NSSelectorFromString("insertNewlineIgnoringFieldEditor:") {
            onNavigate?(NSApp.currentEvent?.modifierFlags.contains(.shift) == true ? -1 : 1)
            return true
        }
        if selector == #selector(NSResponder.moveUp(_:)) { onNavigate?(-1); return true }
        if selector == #selector(NSResponder.moveDown(_:)) { onNavigate?(1); return true }
        return false
    }
    @objc private func optionsChanged() { onChange?(pattern, caseSensitive) }
    @objc private func goPrevious() { onNavigate?(-1) }
    @objc private func goNext() { onNavigate?(1) }
    @objc private func stopSearch() { onStop?() }
    @objc private func closeSearch() { onClose?() }
    @objc private func restartSearch() { onRestart?() }
}
