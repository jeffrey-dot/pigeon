import AppKit

/// The status mark on a row's circle. `working` is a turning arc rather than a static dashed
/// circle, so a busy agent is visible at a glance; turning `blocked` sends out one amber ring
/// that fades, which draws the eye without blinking for as long as the agent waits.
/// Both animations are skipped under Reduce Motion.
final class SidebarStatusBadge: NSView {
    private let image = NSImageView()
    private let arc = CAShapeLayer()
    private let ping = CAShapeLayer()
    private var spinning = false
    private var arcColor: NSColor = SidebarTheme.iconStrong

    override init(frame: NSRect) {
        super.init(frame: frame)
        build()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        build()
    }

    /// `pulse`: this row just turned blocked (not merely redrawn while blocked).
    func show(symbol: String?, color: NSColor, working: Bool, pulse: Bool) {
        isHidden = symbol == nil && !working
        arcColor = color
        if working {
            image.isHidden = true
            arc.isHidden = false
            startSpinning()
        } else {
            stopSpinning()
            arc.isHidden = true
            image.isHidden = symbol == nil
            image.image = symbol.flatMap(Self.symbol)
            image.contentTintColor = color
        }
        updateColors()
        if pulse { sendPing() }
    }

    override func layout() {
        super.layout()
        let inset = bounds.insetBy(dx: 1.5, dy: 1.5)
        arc.frame = bounds
        // Three quarters of a circle; the gap is what makes the rotation readable.
        let path = CGMutablePath()
        path.addArc(
            center: CGPoint(x: bounds.midX, y: bounds.midY), radius: inset.width / 2,
            startAngle: 0, endAngle: .pi * 1.5, clockwise: false)
        arc.path = path
        ping.frame = bounds
        ping.path = CGPath(ellipseIn: inset, transform: nil)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateColors()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        // Core Animation drops animations when a layer leaves the tree.
        if window != nil, !arc.isHidden { spinning = false; startSpinning() }
    }

    private var reduceMotion: Bool {
        NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    }

    private func startSpinning() {
        guard !spinning else { return }
        spinning = true
        guard !reduceMotion else { return }
        let turn = CABasicAnimation(keyPath: "transform.rotation.z")
        turn.fromValue = 0
        turn.toValue = -2 * Double.pi
        turn.duration = 1.1
        turn.repeatCount = .infinity
        turn.isRemovedOnCompletion = false
        arc.add(turn, forKey: "spin")
    }

    private func stopSpinning() {
        spinning = false
        arc.removeAnimation(forKey: "spin")
    }

    private func sendPing() {
        guard !reduceMotion else { return }
        let grow = CABasicAnimation(keyPath: "transform.scale")
        grow.fromValue = 1
        grow.toValue = 2.2
        let fade = CABasicAnimation(keyPath: "opacity")
        fade.fromValue = 0.9
        fade.toValue = 0
        let group = CAAnimationGroup()
        group.animations = [grow, fade]
        group.duration = 0.9
        group.timingFunction = CAMediaTimingFunction(name: .easeOut)
        ping.opacity = 0
        ping.add(group, forKey: "ping")
    }

    /// Layers take `CGColor` snapshots, so they are re-resolved on every appearance change.
    private func updateColors() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            arc.strokeColor = arcColor.cgColor
            ping.strokeColor = SidebarTheme.iconNeedsRing.cgColor
        }
    }

    private static func symbol(_ name: String) -> NSImage? {
        let config = NSImage.SymbolConfiguration(
            pointSize: SidebarMetrics.statusBadgeSize, weight: .bold)
        return NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
    }

    private func build() {
        wantsLayer = true
        layer?.masksToBounds = false
        image.imageScaling = .scaleProportionallyDown
        image.translatesAutoresizingMaskIntoConstraints = false
        addSubview(image)
        NSLayoutConstraint.activate([
            image.leadingAnchor.constraint(equalTo: leadingAnchor),
            image.trailingAnchor.constraint(equalTo: trailingAnchor),
            image.topAnchor.constraint(equalTo: topAnchor),
            image.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        for shape in [arc, ping] {
            shape.fillColor = nil
            shape.lineCap = .round
            layer?.addSublayer(shape)
        }
        arc.lineWidth = 1.8
        ping.lineWidth = 1.5
        ping.opacity = 0
        arc.isHidden = true
    }
}
