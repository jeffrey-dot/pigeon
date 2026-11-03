import AppKit

/// Covers the terminal when the host is gone. A frozen pane with no explanation reads as a
/// hung app; this says what happened and offers the one action that helps.
final class HostStatusView: NSView {
    private let message = NSTextField(labelWithString: "")
    private let button = NSButton(title: "Retry", target: nil, action: nil)

    /// The button was pressed.
    var onAction: (() -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    override var isOpaque: Bool { false }

    func show(message text: String, action: String?) {
        message.stringValue = text
        button.title = action ?? "Retry"
        button.isHidden = action == nil
        isHidden = false
    }

    func hide() {
        isHidden = true
    }

    @objc private func press() {
        onAction?()
    }

    private func build() {
        wantsLayer = true
        // Darker than the grid so the text reads over whatever was on screen when it froze.
        layer?.backgroundColor = SidebarTheme.terminalBackground.withAlphaComponent(0.88).cgColor

        message.font = .systemFont(ofSize: 13)
        message.textColor = SidebarTheme.ink
        message.alignment = .center
        message.maximumNumberOfLines = 6
        message.lineBreakMode = .byWordWrapping
        message.translatesAutoresizingMaskIntoConstraints = false

        button.bezelStyle = .rounded
        button.target = self
        button.action = #selector(press)
        button.translatesAutoresizingMaskIntoConstraints = false

        addSubview(message)
        addSubview(button)
        NSLayoutConstraint.activate([
            message.centerXAnchor.constraint(equalTo: centerXAnchor),
            message.centerYAnchor.constraint(equalTo: centerYAnchor, constant: -16),
            message.widthAnchor.constraint(lessThanOrEqualTo: widthAnchor, constant: -48),
            button.topAnchor.constraint(equalTo: message.bottomAnchor, constant: 16),
            button.centerXAnchor.constraint(equalTo: centerXAnchor),
        ])
        isHidden = true
    }
}
