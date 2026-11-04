import AppKit

/// A nonactivating inspector keeps the terminal window's existing visibility rules intact.
/// Every program-supplied string stays plain text; only the explicitly selected record acks.
final class StatusRecordsPanel: NSPanel {
    private(set) var inspection: StatusInspection
    var onSelect: ((StatusAcknowledgement) -> Void)?
    var onAcknowledge: ((StatusAcknowledgement) -> Void)?
    private let picker = NSPopUpButton(frame: .zero, pullsDown: false)
    private let message = NSTextView(frame: NSRect(x: 0, y: 0, width: 500, height: 180))
    private let note = NSTextField(wrappingLabelWithString: "")
    private let acknowledge = NSButton(title: "Mark This Record as Read", target: nil, action: nil)

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    init(inspection: StatusInspection) {
        self.inspection = inspection
        super.init(contentRect: NSRect(x: 0, y: 0, width: 540, height: 340),
                   styleMask: [.titled, .closable, .nonactivatingPanel], backing: .buffered, defer: false)
        title = "Status Records"
        isReleasedWhenClosed = false
        hidesOnDeactivate = true
        isFloatingPanel = true
        picker.target = self
        picker.action = #selector(selectRecord)
        acknowledge.target = self
        acknowledge.action = #selector(markRead)
        message.isEditable = false
        message.isSelectable = true
        message.isRichText = false
        message.isAutomaticLinkDetectionEnabled = false
        message.font = .systemFont(ofSize: 13)
        message.textContainerInset = NSSize(width: 8, height: 8)
        message.autoresizingMask = [.width]
        message.isVerticallyResizable = true
        message.isHorizontallyResizable = false
        message.textContainer?.widthTracksTextView = true
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.documentView = message
        let stack = NSStackView(views: [picker, scroll, note, acknowledge])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        guard let contentView else { return }
        contentView.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 16),
            stack.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -16),
            stack.topAnchor.constraint(equalTo: contentView.topAnchor, constant: 16),
            stack.bottomAnchor.constraint(equalTo: contentView.bottomAnchor, constant: -16),
            picker.widthAnchor.constraint(equalTo: stack.widthAnchor),
            scroll.widthAnchor.constraint(equalTo: stack.widthAnchor),
            scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 180),
            note.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
    }

    required init?(coder: NSCoder) { return nil }

    func update(records: [PendingStatus], visiblePanes: Set<UInt32>) {
        let changed = inspection.records != records || picker.numberOfItems == 0
        inspection.update(records)
        if changed {
            picker.removeAllItems()
            picker.addItem(withTitle: records.isEmpty ? "No pending records" : "Choose a record…")
            for item in records {
                let state = SidebarModel.subtitle(status: item.status, since: item.agent.since)
                picker.addItem(withTitle: "Pane \(item.target.pane) · \(item.target.record.record) · \(state)")
            }
        }
        if let selected = inspection.selected,
           let index = records.firstIndex(where: { $0.target == selected.target }) {
            picker.selectItem(at: index + 1)
            let text = [selected.agent.app, selected.agent.kind, selected.agent.message]
                .filter { !$0.isEmpty }.joined(separator: "\n\n")
            let displayed = text.isEmpty ? "No message provided." : text
            if message.string != displayed { message.string = displayed }
            acknowledge.isEnabled = selected.canAcknowledge && visiblePanes.contains(selected.target.pane)
            note.stringValue = selected.canAcknowledge
                ? "Read this record, then confirm with its pane visible on the live screen."
                : "This record is live. Respond in its pane; it cannot be marked as read."
        } else {
            picker.selectItem(at: 0)
            message.string = "Choose a record to read its full message."
            acknowledge.isEnabled = false
            note.stringValue = "Only the record you select and confirm is marked as read."
        }
    }

    @objc private func selectRecord() {
        let index = picker.indexOfSelectedItem - 1
        guard inspection.records.indices.contains(index) else { return }
        let target = inspection.records[index].target
        inspection.select(target)
        // Paint the selected text before enabling any confirmation through the app.
        update(records: inspection.records, visiblePanes: [])
        message.scrollToBeginningOfDocument(nil)
        onSelect?(target)
    }

    @objc private func markRead() {
        guard acknowledge.isEnabled, let target = inspection.selected?.target else { return }
        onAcknowledge?(target)
    }
}
