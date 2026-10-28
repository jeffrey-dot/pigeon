import AppKit

/// The heading above one machine's project groups: this Mac's name, or the ssh alias /
/// address the user connected with. Not selectable; it only says where the groups live.
final class SidebarMachineCellView: NSTableCellView {
    private let icon = NSImageView()
    private let name = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    func configure(name text: String) {
        name.stringValue = text
    }

    private func build() {
        let config = NSImage.SymbolConfiguration(pointSize: SidebarMetrics.machineTitleSize, weight: .semibold)
        icon.image = NSImage(systemSymbolName: "desktopcomputer", accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
        icon.contentTintColor = SidebarTheme.gray
        name.font = .systemFont(ofSize: SidebarMetrics.machineTitleSize, weight: .bold)
        name.textColor = SidebarTheme.gray
        name.lineBreakMode = .byTruncatingTail

        for view in [icon, name] {
            addSubview(view)
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        let inset = SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetLeft
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor, constant: inset),
            icon.centerYAnchor.constraint(
                equalTo: bottomAnchor, constant: -SidebarMetrics.machineHeaderHeight / 2),
            name.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: SidebarMetrics.sectionSpacing),
            name.centerYAnchor.constraint(equalTo: icon.centerYAnchor),
            name.trailingAnchor.constraint(
                lessThanOrEqualTo: trailingAnchor,
                constant: -(SidebarMetrics.listSidePadding + SidebarMetrics.sectionInsetRight)),
        ])
    }
}
