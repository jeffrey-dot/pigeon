import AppKit

/// The session list. Rows are derived data: grouping, ordering and copy all come
/// from `SidebarModel`; this class only draws them and reports user intent.
final class SidebarViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private enum Row {
        case group(String, Int)
        case session(SidebarSession)
    }

    private let table = SidebarTable()

    /// NSTableView treats `isGroupRow` rows as non-selectable, so the disclosure
    /// click is handled by the table subclass instead.
    final class SidebarTable: NSTableView {
        var onGroupRowClick: ((Int) -> Void)?

        override func mouseDown(with event: NSEvent) {
            let row = self.row(at: convert(event.locationInWindow, from: nil))
            if row >= 0, let delegate, delegate.tableView?(self, isGroupRow: row) == true {
                onGroupRowClick?(row)
                return
            }
            super.mouseDown(with: event)
        }
    }
    private var rows: [Row] = []
    private var collapsed: Set<String> = []
    private var selected: Int?
    /// Programmatic reloads must not look like the user clicking a row.
    private var suppressSelection = false

    var onSelect: ((Int) -> Void)?
    var onClose: ((Int) -> Void)?
    var onMarkRead: ((Int) -> Void)?
    var onRename: ((Int, String) -> Void)?
    var onToggleGroup: ((String) -> Void)?
    var onReorder: (([Int]) -> Void)?

    override func loadView() {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        table.headerView = nil
        table.rowHeight = 46
        table.style = .sourceList
        table.backgroundColor = .clear
        table.dataSource = self
        table.delegate = self
        table.menu = buildMenu()
        // Without registering the type NSTableView never calls validate/acceptDrop.
        table.registerForDraggedTypes([.string])
        table.setDraggingSourceOperationMask(.move, forLocal: true)
        table.onGroupRowClick = { [weak self] row in
            guard let self, self.rows.indices.contains(row),
                  case let .group(name, _) = self.rows[row] else { return }
            self.onToggleGroup?(name)
        }
        let column = NSTableColumn(identifier: .init("session"))
        column.resizingMask = .autoresizingMask
        table.addTableColumn(column)
        scroll.documentView = table
        view = scroll
    }

    func update(sections: [(group: String, sessions: [SidebarSession])], selected: Int?, collapsed: Set<String>) {
        suppressSelection = true
        defer { suppressSelection = false }
        self.selected = selected
        self.collapsed = collapsed
        rows = sections.flatMap { section -> [Row] in
            var out: [Row] = [.group(section.group, section.sessions.count)]
            if !collapsed.contains(section.group) {
                out.append(contentsOf: section.sessions.map(Row.session))
            }
            return out
        }
        table.reloadData()
        if let selected, let index = rows.firstIndex(where: {
            if case let .session(session) = $0 { return session.id == selected }
            return false
        }) {
            table.selectRowIndexes([index], byExtendingSelection: false)
        }
    }

    // MARK: - table

    private func session(at row: Int) -> SidebarSession? {
        guard rows.indices.contains(row), case let .session(session) = rows[row] else { return nil }
        return session
    }

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool { false }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool { true }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch rows[row] {
        case let .group(name, count):
            let label = rowLabel("\(collapsed.contains(name) ? "\u{25B8}" : "\u{25BE}") \(name)", size: 11, bold: true)
            label.toolTip = "\(count) session(s) — click to collapse"
            let view = NSTableCellView()
            view.addSubview(label)
            label.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                label.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 4),
                label.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            ])
            return view
        case let .session(session):
            return sessionCell(session)
        }
    }

    private func rowLabel(_ text: String, size: CGFloat, bold: Bool) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = .systemFont(ofSize: size, weight: bold ? .semibold : .regular)
        label.lineBreakMode = .byTruncatingTail
        return label
    }

    private func sessionCell(_ session: SidebarSession) -> NSView {
        let view = NSTableCellView()
        let title = rowLabel(session.title, size: 13, bold: false)
        let subtitle = rowLabel(SidebarModel.rowSubtitle(for: session), size: 11, bold: false)
        subtitle.textColor = .secondaryLabelColor
        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.cornerRadius = 3
        dot.layer?.backgroundColor = statusColor(session).cgColor
        for subview in [dot, title, subtitle] { view.addSubview(subview); subview.translatesAutoresizingMaskIntoConstraints = false }
        NSLayoutConstraint.activate([
            dot.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 6),
            dot.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            dot.widthAnchor.constraint(equalToConstant: 6),
            dot.heightAnchor.constraint(equalToConstant: 6),
            title.leadingAnchor.constraint(equalTo: dot.trailingAnchor, constant: 8),
            title.topAnchor.constraint(equalTo: view.topAnchor, constant: 7),
            title.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -6),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 2),
            subtitle.trailingAnchor.constraint(equalTo: title.trailingAnchor),
        ])
        if session.unread > 0 || session.status == .blocked || session.status == .error {
            view.layer?.backgroundColor = NSColor.controlAccentColor.withAlphaComponent(0.08).cgColor
        }
        return view
    }

    private func statusColor(_ session: SidebarSession) -> NSColor {
        switch session.status {
        case .blocked: return .systemOrange
        case .error: return .systemRed
        case .done: return .systemGreen
        case .working: return .systemBlue
        case .idle: return session.unread > 0 ? .systemYellow : .tertiaryLabelColor
        }
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelection, rows.indices.contains(table.selectedRow) else { return }
        switch rows[table.selectedRow] {
        case let .group(name, _):
            table.deselectAll(nil)
            onToggleGroup?(name)
        case let .session(session):
            onSelect?(session.id)
        }
    }

    // MARK: - context menu

    private func buildMenu() -> NSMenu {
        let menu = NSMenu()
        menu.addItem(withTitle: "Close Session", action: #selector(closeClicked), keyEquivalent: "")
        menu.addItem(withTitle: "Mark as Read", action: #selector(markReadClicked), keyEquivalent: "")
        menu.addItem(withTitle: "Rename…", action: #selector(renameClicked), keyEquivalent: "")
        for item in menu.items { item.target = self }
        return menu
    }

    @objc private func closeClicked() {
        if let session = session(at: table.clickedRow) { onClose?(session.id) }
    }

    @objc private func markReadClicked() {
        if let session = session(at: table.clickedRow) { onMarkRead?(session.id) }
    }

    @objc private func renameClicked() {
        guard let session = session(at: table.clickedRow) else { return }
        let alert = NSAlert()
        alert.messageText = "Rename session"
        let field = NSTextField(string: session.title)
        field.frame = NSRect(x: 0, y: 0, width: 240, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if !name.isEmpty { onRename?(session.id, name) }
    }

    // MARK: - drag to reorder

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        guard let session = session(at: row) else { return nil }
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: sidebar drag row=\(row) id=\(session.id)\n".utf8))
        }
        let item = NSPasteboardItem()
        item.setString(String(session.id), forType: .string)
        return item
    }

    func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int, proposedDropOperation op: NSTableView.DropOperation) -> NSDragOperation {
        .move
    }

    func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation op: NSTableView.DropOperation) -> Bool {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: sidebar drop row=\(row)\n".utf8))
        }
        guard let text = info.draggingPasteboard.string(forType: .string), let id = Int(text) else { return false }
        var ids = rows.compactMap { row -> Int? in
            if case let .session(session) = row { return session.id }
            return nil
        }
        ids.removeAll { $0 == id }
        let target = session(at: row)?.id ?? session(at: max(row - 1, 0))?.id
        let index = target.flatMap { ids.firstIndex(of: $0) } ?? ids.count
        ids.insert(id, at: min(index, ids.count))
        onReorder?(ids)
        return true
    }
}
