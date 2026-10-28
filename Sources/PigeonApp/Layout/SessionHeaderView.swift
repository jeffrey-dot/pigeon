import AppKit

/// Right-pane header from `docs/ui.md` §3: session name, cwd, and the status
/// badge (`Working · 1m`) that repeats the selected row's state in words.
final class SessionHeaderView: NSView {
    private let title = NSTextField(labelWithString: "")
    private let cwd = NSTextField(labelWithString: "")
    private let badge = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    func update(title name: String, cwd path: String, badge text: String) {
        title.stringValue = name
        cwd.stringValue = path
        badge.stringValue = text
    }

    private func build() {
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        cwd.font = .systemFont(ofSize: 11)
        cwd.textColor = .secondaryLabelColor
        badge.font = .systemFont(ofSize: 11)
        badge.textColor = .secondaryLabelColor
        badge.alignment = .right
        for label in [title, cwd, badge] {
            label.lineBreakMode = .byTruncatingTail
            addSubview(label)
            label.translatesAutoresizingMaskIntoConstraints = false
        }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            title.centerYAnchor.constraint(equalTo: centerYAnchor),
            cwd.leadingAnchor.constraint(equalTo: title.trailingAnchor, constant: 8),
            cwd.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            badge.centerYAnchor.constraint(equalTo: centerYAnchor),
            cwd.trailingAnchor.constraint(lessThanOrEqualTo: badge.leadingAnchor, constant: -8),
        ])
    }
}
