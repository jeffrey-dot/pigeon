import AppKit

/// Harp's glass tint over a native sidebar material. Reduce Transparency uses the opaque
/// sidebar token so the window stays readable without relying on the desktop behind it.
final class SidebarBackgroundView: NSVisualEffectView {
    private let tint = NSView()
    private var displayObserver: NSObjectProtocol?

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    private func build() {
        material = .sidebar
        blendingMode = .behindWindow
        state = .followsWindowActiveState
        tint.wantsLayer = true
        tint.translatesAutoresizingMaskIntoConstraints = false
        addSubview(tint)
        NSLayoutConstraint.activate([
            tint.leadingAnchor.constraint(equalTo: leadingAnchor),
            tint.trailingAnchor.constraint(equalTo: trailingAnchor),
            tint.topAnchor.constraint(equalTo: topAnchor),
            tint.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        displayObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil, queue: .main
        ) { [weak self] _ in self?.applyAppearance() }
        applyAppearance()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyAppearance()
    }

    private func applyAppearance() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            let color = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
                ? SidebarTheme.sidebar : SidebarTheme.glassTint
            tint.layer?.backgroundColor = color.cgColor
        }
    }

    deinit {
        if let displayObserver { NSWorkspace.shared.notificationCenter.removeObserver(displayObserver) }
    }
}
