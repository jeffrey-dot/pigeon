import AppKit

/// The right pane is one card in harp (`docs/ui.md` §7.1): 10pt radius, a 0.5pt edge and a
/// soft shadow, inset 8pt from the window's top/right/bottom edge so the sidebar's surface
/// shows through the gutter. The left edge stays flush with the divider.
final class CardView: NSView {
    /// Rounded and clipping, so the terminal's own background cannot square off the corners.
    /// The shadow lives on the outer view, which stays unclipped.
    private let surface = NSView()

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    private func build() {
        wantsLayer = true
        addSubview(surface)
        surface.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            surface.topAnchor.constraint(equalTo: topAnchor),
            surface.leadingAnchor.constraint(equalTo: leadingAnchor),
            surface.trailingAnchor.constraint(equalTo: trailingAnchor),
            surface.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        applyAppearance()
    }

    /// Subviews go inside the rounded, clipping surface.
    override func addSubview(_ view: NSView) {
        if view === surface || surface.superview == nil && view !== self {
            super.addSubview(view)
        } else {
            surface.addSubview(view)
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyAppearance()
    }

    override var isOpaque: Bool { false }

    /// The colour the inner surface is painted with, for tests: layer colours are snapshots,
    /// so this is how the appearance switch is verified without a screen.
    var surfaceColorForTesting: NSColor? {
        surface.layer?.backgroundColor.flatMap { NSColor(cgColor: $0)?.usingColorSpace(.sRGB) }
    }

    /// Layer colors are snapshots, so the dynamic tokens are resolved here on every change.
    private func applyAppearance() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            surface.wantsLayer = true
            guard let layer, let inner = surface.layer else { return }
            layer.shadowColor = SidebarTheme.cardShadow.cgColor
            layer.shadowOpacity = 1
            layer.shadowRadius = SidebarMetrics.cardShadowBlur / 2
            layer.shadowOffset = CGSize(width: 0, height: -SidebarMetrics.cardShadowOffsetY)
            inner.cornerRadius = SidebarMetrics.cardRadius
            inner.cornerCurve = .continuous
            inner.masksToBounds = true
            inner.borderWidth = SidebarMetrics.cardEdgeWidth
            inner.borderColor = SidebarTheme.cardEdge.cgColor
            inner.backgroundColor = SidebarTheme.terminalBackground.cgColor
        }
    }
}
