import AppKit
import PigeonCore

/// Holds one `PaneView` per pane, draws the dividers, and turns mouse gestures
/// into `resize_split` / `focus` requests. Geometry comes from the core.
final class SplitContainerView: NSView {
    private(set) var model: LayoutModel = .empty
    private var panes: [UInt32: PaneView] = [:]
    private var draggingSplit: UInt32?

    var onSplit: ((PgAxis) -> Void)?
    var onClose: (() -> Void)?
    var onFocus: ((UInt32) -> Void)?
    var onResizeSplit: ((UInt32, Double) -> Void)?
    var onPaneInput: ((UInt32, [UInt8]) -> Void)?
    var onPaneResize: ((UInt32, UInt16, UInt16) -> Void)?
    /// The pane needs a fresh scrollback: the owner re-attaches that pane (protocol §3.3).
    var onPaneHistory: ((UInt32) -> Void)?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    private let dividerThickness: CGFloat = 1
    private let grabTolerance: CGFloat = 4

    func apply(_ model: LayoutModel) {
        self.model = model
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(
                Data("pigeon: layout \(model.panes.count) panes focus=\(model.focus)\n".utf8)
            )
        }
        let live = Set(model.panes.map(\.id))
        for (paneID, view) in panes where !live.contains(paneID) {
            view.removeFromSuperview()
            panes.removeValue(forKey: paneID)
        }
        for pane in model.panes {
            let view = panes[pane.id] ?? makePane(pane.id)
            view.frame = frame(for: pane.rect)
            panes[pane.id] = view
        }
        if let focused = panes[model.focus], window?.firstResponder !== focused {
            window?.makeFirstResponder(focused)
        }
        needsDisplay = true
    }

    private func makePane(_ id: UInt32) -> PaneView {
        let view = PaneView(frame: .zero)
        view.paneID = id
        view.onInput = { [weak self] bytes in self?.onPaneInput?(id, bytes) }
        view.onResize = { [weak self] cols, rows in self?.onPaneResize?(id, cols, rows) }
        view.onNeedHistory = { [weak self] pane in self?.onPaneHistory?(pane) }
        addSubview(view)
        return view
    }

    func paneView(_ id: UInt32) -> PaneView? { panes[id] }

    /// Makes `id` the keyboard target; the host is told separately via `onFocus`.
    func focusPane(_ id: UInt32) {
        guard let view = panes[id] else { return }
        window?.makeFirstResponder(view)
    }

    /// Applies a snapshot message to the right pane.
    func applySnapshot(_ snapshot: UnsafePointer<PgSnapshot>) -> Bool {
        let id = snapshot.pointee.pane_id
        guard let view = panes[id] else { return false }
        view.apply(snapshot: snapshot)
        return true
    }

    private func frame(for rect: CGRect) -> NSRect {
        NSRect(
            x: rect.minX * bounds.width,
            y: rect.minY * bounds.height,
            width: max(rect.width * bounds.width - dividerThickness, 1),
            height: max(rect.height * bounds.height - dividerThickness, 1)
        )
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        apply(model)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor(calibratedWhite: 0.25, alpha: 1).setFill()
        for divider in model.dividers {
            switch divider.axis {
            case PG_AXIS_VERTICAL:
                let x = (divider.box.minX + divider.box.width * divider.ratio) * bounds.width
                NSRect(x: x, y: divider.box.minY * bounds.height, width: dividerThickness,
                       height: divider.box.height * bounds.height).fill()
            default:
                let y = (divider.box.minY + divider.box.height * divider.ratio) * bounds.height
                NSRect(x: divider.box.minX * bounds.width, y: y,
                       width: divider.box.width * bounds.width, height: dividerThickness).fill()
            }
        }
    }

    // MARK: - mouse

    private func normalized(_ event: NSEvent) -> CGPoint {
        let point = convert(event.locationInWindow, from: nil)
        guard bounds.width > 0, bounds.height > 0 else { return .zero }
        return CGPoint(x: point.x / bounds.width, y: point.y / bounds.height)
    }

    override func mouseDown(with event: NSEvent) {
        let point = normalized(event)
        let tolerance = Double(grabTolerance / max(bounds.width, 1))
        if let divider = model.divider(at: point, tolerance: tolerance) {
            draggingSplit = divider.splitID
            NSCursor.resizeLeftRight.set()
            return
        }
        if let pane = model.pane(at: point) {
            focusPane(pane.id)
            onFocus?(pane.id)
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard let splitID = draggingSplit,
              let divider = model.dividers.first(where: { $0.splitID == splitID })
        else { return }
        onResizeSplit?(splitID, model.proposedRatio(divider, at: normalized(event)))
    }

    override func mouseUp(with event: NSEvent) {
        draggingSplit = nil
        NSCursor.arrow.set()
    }

    // MARK: - menu actions

    @objc func splitVertical(_ sender: Any?) { onSplit?(PG_AXIS_VERTICAL) }
    @objc func splitHorizontal(_ sender: Any?) { onSplit?(PG_AXIS_HORIZONTAL) }
    @objc func closeFocused(_ sender: Any?) { onClose?() }
    @objc func focusPrevious(_ sender: Any?) {
        if let next = model.nextFocus(forward: false) {
            focusPane(next)
            onFocus?(next)
        }
    }
    @objc func focusNext(_ sender: Any?) {
        if let next = model.nextFocus(forward: true) {
            focusPane(next)
            onFocus?(next)
        }
    }

    // MARK: - shortcuts

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.modifierFlags.contains(.command) else { return false }
        let shift = event.modifierFlags.contains(.shift)
        switch event.charactersIgnoringModifiers?.lowercased() {
        case "d":
            onSplit?(shift ? PG_AXIS_HORIZONTAL : PG_AXIS_VERTICAL)
            return true
        case "w":
            onClose?()
            return true
        case "[":
            if let next = model.nextFocus(forward: false) {
                focusPane(next)
                onFocus?(next)
            }
            return true
        case "]":
            if let next = model.nextFocus(forward: true) {
                focusPane(next)
                onFocus?(next)
            }
            return true
        default:
            return false
        }
    }
}
