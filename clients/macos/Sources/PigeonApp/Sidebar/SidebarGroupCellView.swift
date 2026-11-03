import AppKit

/// A section header: project name, member count, `N needs you`, and — like harp — a `+` that
/// appears while the pointer is over the header and creates a session in that project.
final class SidebarGroupCellView: NSTableCellView {
    var onNewSession: (() -> Void)?

    private let name = NSTextField(labelWithString: "")
    private let count = NSTextField(labelWithString: "")
    private let needs = NSTextField(labelWithString: "")
    private let add = NSButton()
    private var tracking: NSTrackingArea?
    private var needsTrailing: NSLayoutConstraint!

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    func configure(group: String, count total: Int, needs wanted: Int) {
        name.stringValue = SidebarModel.displayGroupName(group)
        name.toolTip = name.stringValue
        count.stringValue = "\(total)"
        needs.stringValue = wanted > 0 ? SidebarTheme.needsLabel(wanted) : ""
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        effectiveAppearance.performAsCurrentDrawingAppearance {
            add.layer?.backgroundColor = SidebarTheme.hover.cgColor
        }
    }

    private func build() {
        name.font = .systemFont(ofSize: SidebarMetrics.sectionTitleSize, weight: .semibold)
        name.textColor = SidebarTheme.sectionName
        name.lineBreakMode = .byTruncatingTail
        name.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        count.font = .monospacedDigitSystemFont(ofSize: SidebarMetrics.sectionCountSize, weight: .regular)
        count.textColor = SidebarTheme.sectionCount
        needs.font = .monospacedDigitSystemFont(ofSize: SidebarMetrics.needsLabelSize, weight: .regular)
        needs.textColor = SidebarTheme.needsText

        add.isBordered = false
        let config = NSImage.SymbolConfiguration(pointSize: 10, weight: .semibold)
        add.image = NSImage(systemSymbolName: "plus", accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
        add.contentTintColor = SidebarTheme.gray
        add.target = self
        add.action = #selector(addClicked)
        add.wantsLayer = true
        add.layer?.cornerRadius = SidebarMetrics.groupActionRadius
        add.layer?.backgroundColor = SidebarTheme.hover.cgColor
        add.isHidden = true
        add.setAccessibilityLabel("New Session in Project")

        for view in [name, count, needs, add] {
            addSubview(view)
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        needsTrailing = needs.trailingAnchor.constraint(
            equalTo: trailingAnchor,
            constant: -(SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetRight))
        NSLayoutConstraint.activate([
            name.leadingAnchor.constraint(
                equalTo: leadingAnchor,
                constant: SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetLeft),
            name.centerYAnchor.constraint(
                equalTo: bottomAnchor, constant: -SidebarMetrics.sectionHeaderHeight / 2),
            count.leadingAnchor.constraint(
                equalTo: name.trailingAnchor, constant: SidebarMetrics.sectionSpacing),
            count.firstBaselineAnchor.constraint(equalTo: name.firstBaselineAnchor),
            add.trailingAnchor.constraint(
                equalTo: trailingAnchor,
                constant: -(SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetRight)),
            add.centerYAnchor.constraint(equalTo: name.centerYAnchor),
            add.widthAnchor.constraint(equalToConstant: SidebarMetrics.groupActionSize),
            add.heightAnchor.constraint(equalToConstant: SidebarMetrics.groupActionSize),
            needsTrailing,
            needs.firstBaselineAnchor.constraint(equalTo: name.firstBaselineAnchor),
            needs.leadingAnchor.constraint(greaterThanOrEqualTo: count.trailingAnchor, constant: 6),
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
        add.isHidden = false
        needsTrailing.constant = -(SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetRight
            + SidebarMetrics.groupActionSize + SidebarMetrics.sectionSpacing)
    }

    override func mouseExited(with event: NSEvent) {
        add.isHidden = true
        needsTrailing.constant = -(SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetRight)
    }

    @objc private func addClicked() { onNewSession?() }
}
