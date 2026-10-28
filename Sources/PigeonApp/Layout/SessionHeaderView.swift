import AppKit

/// The card header from harp (`docs/ui.md` §7.3): session name, path, and the state
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

    func update(title name: String, cwd: String, badge text: String) {
        title.stringValue = name
        path.stringValue = cwd
        state.stringValue = text
    }

    /// The path gives way from its head, the way harp clips a long working directory.
    private func build() {
        title.font = .systemFont(ofSize: SidebarMetrics.cardTitleSize, weight: .semibold)
        title.textColor = SidebarTheme.cardInk
        path.font = .systemFont(ofSize: SidebarMetrics.cardPathSize)
        path.textColor = SidebarTheme.cardGray
        path.lineBreakMode = .byTruncatingHead
        state.font = .systemFont(ofSize: SidebarMetrics.cardStateSize)
        state.textColor = SidebarTheme.cardGray
        state.alignment = .right

        for label in [title, path, state] {
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
            path.leadingAnchor.constraint(
                equalTo: title.trailingAnchor, constant: SidebarMetrics.cardHeaderSpacing),
            path.centerYAnchor.constraint(equalTo: centerYAnchor),
            path.trailingAnchor.constraint(
                lessThanOrEqualTo: state.leadingAnchor, constant: -SidebarMetrics.cardHeaderSpacing),
            state.trailingAnchor.constraint(
                equalTo: trailingAnchor, constant: -SidebarMetrics.cardHeaderPadRight),
            state.centerYAnchor.constraint(equalTo: centerYAnchor),
            divider.leadingAnchor.constraint(equalTo: leadingAnchor),
            divider.trailingAnchor.constraint(equalTo: trailingAnchor),
            divider.bottomAnchor.constraint(equalTo: bottomAnchor),
            divider.heightAnchor.constraint(equalToConstant: SidebarMetrics.cardEdgeWidth),
        ])
    }
}
