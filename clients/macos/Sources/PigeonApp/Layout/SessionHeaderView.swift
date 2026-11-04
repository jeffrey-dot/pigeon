import AppKit

/// The card header from Harp (`docs/ui.md` §2): session name, path, and the state
/// wording on the right, 40pt tall over a 0.5pt divider.
final class SessionHeaderView: NSView {
    private let title = NSTextField(labelWithString: "")
    private let path = NSTextField(labelWithString: "")
    private let state = NSTextField(labelWithString: "")
    private let divider = DividerView()
    private var titleLeading: NSLayoutConstraint!

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    /// harp pads the header past the traffic lights when the sidebar is hidden.
    func setSidebarCollapsed(_ collapsed: Bool) {
        titleLeading.constant = collapsed
            ? SidebarMetrics.cardHeaderPadClosed
            : SidebarMetrics.cardHeaderPadOpen
    }

    func update(title name: String, cwd: String, badge text: String, detail: String? = nil) {
        title.stringValue = name
        path.stringValue = cwd
        state.stringValue = text
        title.toolTip = name
        path.toolTip = cwd
        state.toolTip = [text, detail ?? ""].filter { !$0.isEmpty }.joined(separator: "\n")
    }

    /// The path gives way from its head, the way harp clips a long working directory.
    private func build() {
        title.font = .systemFont(ofSize: SidebarMetrics.cardTitleSize, weight: .semibold)
        title.textColor = SidebarTheme.ink
        path.font = .systemFont(ofSize: SidebarMetrics.cardPathSize)
        path.textColor = SidebarTheme.gray
        path.lineBreakMode = .byTruncatingHead
        state.font = .monospacedDigitSystemFont(ofSize: SidebarMetrics.cardStateSize, weight: .regular)
        state.textColor = SidebarTheme.gray
        state.alignment = .right
        title.lineBreakMode = .byTruncatingTail
        state.lineBreakMode = .byTruncatingTail
        title.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)
        path.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        state.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)

        for label in [title, path, state] {
            label.maximumNumberOfLines = 1
            label.cell?.wraps = false
            addSubview(label)
            label.translatesAutoresizingMaskIntoConstraints = false
        }
        addSubview(divider)
        divider.translatesAutoresizingMaskIntoConstraints = false
        titleLeading = title.leadingAnchor.constraint(
            equalTo: leadingAnchor, constant: SidebarMetrics.cardHeaderPadOpen)
        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: SidebarMetrics.cardHeaderHeight),
            titleLeading,
            title.centerYAnchor.constraint(equalTo: centerYAnchor),
            title.widthAnchor.constraint(greaterThanOrEqualToConstant: 0),
            path.leadingAnchor.constraint(
                equalTo: title.trailingAnchor, constant: SidebarMetrics.cardHeaderSpacing),
            path.centerYAnchor.constraint(equalTo: centerYAnchor),
            path.widthAnchor.constraint(greaterThanOrEqualToConstant: 0),
            path.trailingAnchor.constraint(
                lessThanOrEqualTo: state.leadingAnchor, constant: -SidebarMetrics.cardHeaderSpacing),
            state.trailingAnchor.constraint(
                equalTo: trailingAnchor, constant: -SidebarMetrics.cardHeaderPadRight),
            state.centerYAnchor.constraint(equalTo: centerYAnchor),
            state.widthAnchor.constraint(lessThanOrEqualTo: widthAnchor, multiplier: 0.45),
            divider.leadingAnchor.constraint(equalTo: leadingAnchor),
            divider.trailingAnchor.constraint(equalTo: trailingAnchor),
            divider.bottomAnchor.constraint(equalTo: bottomAnchor),
            divider.heightAnchor.constraint(equalToConstant: SidebarMetrics.cardEdgeWidth),
        ])
    }
}
