import AppKit

/// A compact session row: name, age, and a trailing status slot. Agent details stay in the
/// tooltip so a long task or permission question cannot change the list's rhythm.
final class SidebarRowCellView: NSTableCellView {
    var onClose: (() -> Void)?

    private let badge = SidebarStatusBadge()
    private let title = NSTextField(labelWithString: "")
    private let age = NSTextField(labelWithString: "")
    private let close = SidebarHeaderButton()
    private var tracking: NSTrackingArea?
    private var hovered = false
    private var hasStatus = false

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    func configure(with session: SidebarSession, pulse: Bool = false) {
        title.stringValue = SidebarModel.rowTitle(for: session)
        title.font = .systemFont(
            ofSize: SidebarMetrics.rowTitleSize,
            weight: session.unread > 0 ? .semibold : .regular)
        title.textColor = SidebarTheme.ink
        age.stringValue = SidebarModel.ageText(for: session)
        age.textColor = SidebarTheme.gray
        let detail = SidebarModel.detailLine(for: session)
        toolTip = [title.stringValue, session.cwd, detail ?? "", age.stringValue]
            .filter { !$0.isEmpty }.joined(separator: "\n")
        title.toolTip = toolTip
        age.toolTip = age.stringValue
        close.setAccessibilityLabel("Close \(title.stringValue)")

        let symbol = session.reported ? SidebarModel.statusSymbol(for: session) : nil
        hasStatus = symbol != nil
        badge.show(
            symbol: symbol, color: statusColor(session),
            working: session.reported && session.status == .working, pulse: pulse)
        badge.setAccessibilityLabel(SidebarModel.headerBadge(for: session))
        updateHover()
    }

    private func statusColor(_ session: SidebarSession) -> NSColor {
        switch session.status {
        case .blocked: return SidebarTheme.iconNeedsRing
        case .error: return SidebarTheme.error
        case .done: return session.unread > 0 ? SidebarTheme.iconStrong : SidebarTheme.ring
        case .working: return SidebarTheme.iconStrong
        case .idle: return SidebarTheme.ring
        }
    }

    private func build() {
        title.identifier = .init("session.title")
        age.identifier = .init("session.age")
        badge.identifier = .init("session.status")
        title.lineBreakMode = .byTruncatingTail
        title.maximumNumberOfLines = 1
        title.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        age.font = .monospacedDigitSystemFont(ofSize: SidebarMetrics.rowTimeSize, weight: .regular)
        age.lineBreakMode = .byTruncatingMiddle
        age.maximumNumberOfLines = 1
        age.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)

        close.image = NSImage(systemSymbolName: "xmark", accessibilityDescription: nil)?
            .withSymbolConfiguration(.init(pointSize: 9, weight: .semibold))
        close.contentTintColor = SidebarTheme.gray
        close.target = self
        close.action = #selector(closeClicked)
        close.isHidden = true

        for view in [title, age, badge, close] {
            addSubview(view)
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        let leading = SidebarMetrics.listSidePadding + SidebarMetrics.rowInsetLeft
        let trailing = SidebarMetrics.listSidePadding + SidebarMetrics.rowInsetRight
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: leading),
            title.centerYAnchor.constraint(equalTo: centerYAnchor),
            title.trailingAnchor.constraint(
                lessThanOrEqualTo: age.leadingAnchor, constant: -SidebarMetrics.rowSpacing),
            title.widthAnchor.constraint(greaterThanOrEqualToConstant: 24),
            age.trailingAnchor.constraint(equalTo: badge.leadingAnchor, constant: -SidebarMetrics.rowSpacing),
            age.centerYAnchor.constraint(equalTo: centerYAnchor),
            age.widthAnchor.constraint(lessThanOrEqualTo: widthAnchor, multiplier: 0.45),
            badge.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -trailing),
            badge.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.widthAnchor.constraint(equalToConstant: SidebarMetrics.statusGlyphSlot),
            badge.heightAnchor.constraint(equalToConstant: SidebarMetrics.statusGlyphSlot),
            close.centerXAnchor.constraint(equalTo: badge.centerXAnchor),
            close.centerYAnchor.constraint(equalTo: badge.centerYAnchor),
            close.widthAnchor.constraint(equalToConstant: SidebarMetrics.rowCloseSize),
            close.heightAnchor.constraint(equalToConstant: SidebarMetrics.rowCloseSize),
        ])
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
            owner: self)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) {
        hovered = true
        updateHover()
    }

    override func mouseExited(with event: NSEvent) {
        hovered = false
        updateHover()
    }

    private func updateHover() {
        // The close button replaces the status; the age and title never move on hover.
        badge.isHidden = hovered || !hasStatus
        close.isHidden = !hovered
    }

    @objc private func closeClicked() { onClose?() }
}
