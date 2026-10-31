import AppKit

/// One session row, shaped like harp's: a 24pt circle with the program's glyph, the status on
/// the circle's corner, the title, and the age on the right — which the pointer swaps for a
/// close chip (`VT` `row_close`).
final class SidebarRowCellView: NSTableCellView {
    var onClose: (() -> Void)?

    private let circle = NSView()
    private let glyph = NSImageView()
    private let badge = NSImageView()
    private let title = NSTextField(labelWithString: "")
    /// The agent's task or question, under the title; hidden for a one-line row.
    private let detail = NSTextField(labelWithString: "")
    private let age = NSTextField(labelWithString: "")
    /// Title centred alone, or lifted to make room for `detail`.
    private var titleCentered: NSLayoutConstraint!
    private var titleRaised: NSLayoutConstraint!
    private let close = NSButton()
    private var tracking: NSTrackingArea?

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    func configure(with session: SidebarSession) {
        let tint = SidebarTheme.appTints[
            SidebarModel.tintIndex(cwd: session.cwd, buckets: SidebarTheme.appTints.count)]
        circle.layer?.backgroundColor = tint.cgColor
        glyph.image = Self.symbol(
            SidebarModel.appKind(session.app).symbol,
            size: SidebarMetrics.statusGlyphSize,
            weight: .semibold
        )
        // The circle is a mid-tone in light and a pastel in dark, so a dark glyph reads on both.
        glyph.contentTintColor = NSColor.black.withAlphaComponent(0.72)

        let unread = session.unread > 0
        title.stringValue = SidebarModel.rowTitle(for: session)
        title.font = .systemFont(ofSize: SidebarMetrics.rowTitleSize, weight: unread ? .semibold : .regular)
        title.textColor = SidebarTheme.ink
        age.stringValue = SidebarModel.ageText(for: session)
        age.font = .systemFont(ofSize: SidebarMetrics.rowTimeSize)
        age.textColor = session.status == .blocked ? SidebarTheme.needsAccent : SidebarTheme.gray

        // Program-controlled text: a plain label, one line, never parsed (AGENTS.md §8).
        if let line = SidebarModel.detailLine(for: session) {
            detail.stringValue = line
            detail.textColor = session.status == .blocked ? SidebarTheme.needsAccent : SidebarTheme.gray
            detail.isHidden = false
            titleCentered.isActive = false
            titleRaised.isActive = true
        } else {
            detail.isHidden = true
            titleRaised.isActive = false
            titleCentered.isActive = true
        }

        // Status on the circle's corner, the way harp draws it; no state → no badge.
        if let symbol = SidebarModel.statusSymbol(for: session) {
            badge.image = Self.symbol(symbol, size: SidebarMetrics.statusBadgeSize, weight: .bold)
            badge.contentTintColor = statusColor(session)
            badge.isHidden = false
        } else {
            badge.isHidden = true
        }
        close.isHidden = true
        age.isHidden = false
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

    private static func symbol(_ name: String, size: CGFloat, weight: NSFont.Weight) -> NSImage? {
        let config = NSImage.SymbolConfiguration(pointSize: size, weight: weight)
        return NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
    }

    private func build() {
        circle.wantsLayer = true
        circle.layer?.cornerRadius = SidebarMetrics.rowIconSize / 2
        addSubview(circle)

        for view in [glyph, badge] {
            addSubview(view)
            view.imageScaling = .scaleProportionallyDown
        }
        addSubview(title)
        addSubview(detail)
        detail.font = .systemFont(ofSize: SidebarMetrics.rowDetailSize)
        detail.lineBreakMode = .byTruncatingTail
        detail.maximumNumberOfLines = 1
        detail.cell?.truncatesLastVisibleLine = true
        title.lineBreakMode = .byTruncatingTail
        addSubview(age)
        addSubview(close)

        close.isBordered = false
        close.image = Self.symbol("xmark", size: 9, weight: .bold)
        close.contentTintColor = SidebarTheme.gray
        close.target = self
        close.action = #selector(closeClicked)
        close.wantsLayer = true
        close.layer?.cornerRadius = SidebarMetrics.rowCloseRadius
        close.layer?.backgroundColor = SidebarTheme.hover.cgColor

        for view in [circle, glyph, badge, title, detail, age, close] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        let leading = SidebarMetrics.listSidePadding + SidebarMetrics.rowInsetLeft
        let trailing = SidebarMetrics.listSidePadding + SidebarMetrics.rowInsetRight
        titleCentered = title.centerYAnchor.constraint(equalTo: centerYAnchor)
        titleRaised = title.bottomAnchor.constraint(equalTo: centerYAnchor, constant: 1)
        titleCentered.isActive = true
        NSLayoutConstraint.activate([
            circle.leadingAnchor.constraint(equalTo: leadingAnchor, constant: leading),
            circle.centerYAnchor.constraint(equalTo: centerYAnchor),
            circle.widthAnchor.constraint(equalToConstant: SidebarMetrics.rowIconSize),
            circle.heightAnchor.constraint(equalToConstant: SidebarMetrics.rowIconSize),
            glyph.centerXAnchor.constraint(equalTo: circle.centerXAnchor),
            glyph.centerYAnchor.constraint(equalTo: circle.centerYAnchor),
            glyph.widthAnchor.constraint(equalToConstant: 14),
            glyph.heightAnchor.constraint(equalToConstant: 14),
            // Corner of the circle, like harp's `render_with_optional_status_badge`.
            badge.trailingAnchor.constraint(equalTo: circle.trailingAnchor, constant: 3),
            badge.bottomAnchor.constraint(equalTo: circle.bottomAnchor, constant: 2),
            badge.widthAnchor.constraint(equalToConstant: SidebarMetrics.statusBadgeSize),
            badge.heightAnchor.constraint(equalToConstant: SidebarMetrics.statusBadgeSize),
            title.leadingAnchor.constraint(
                equalTo: circle.trailingAnchor, constant: SidebarMetrics.rowIconGap),
            title.trailingAnchor.constraint(lessThanOrEqualTo: age.leadingAnchor, constant: -6),
            detail.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            detail.topAnchor.constraint(equalTo: centerYAnchor, constant: 1),
            detail.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -trailing),
            age.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -trailing),
            age.centerYAnchor.constraint(equalTo: centerYAnchor),
            close.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -trailing),
            close.centerYAnchor.constraint(equalTo: centerYAnchor),
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
            owner: self
        )
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) {
        age.isHidden = true
        close.isHidden = false
    }

    override func mouseExited(with event: NSEvent) {
        age.isHidden = false
        close.isHidden = true
    }

    @objc private func closeClicked() { onClose?() }
}
