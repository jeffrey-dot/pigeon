import AppKit

/// The session list: project groups as headers, one row per session. Rows are
/// derived data (grouping, order, copy) from `SidebarModel`; this class only
/// draws them and reports user intent.
final class SidebarViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private enum Row {
        case group(String, Int)
        case session(SidebarSession)
    }

    private let table = NSTableView()
    private var rows: [Row] = []
    private var selected: Int?
    /// Programmatic reloads must not look like the user clicking a row.
    private var suppressSelection = false

    var onSelect: ((Int) -> Void)?
    var onClose: ((Int) -> Void)?
    var onMarkRead: ((Int) -> Void)?
    var onRename: ((Int, String) -> Void)?
    var onRenameGroup: ((String, String) -> Void)?
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
        table.registerForDraggedTypes([.string])
        table.setDraggingSourceOperationMask(.move, forLocal: true)
        let column = NSTableColumn(identifier: .init("session"))
        column.resizingMask = .autoresizingMask
        table.addTableColumn(column)
        scroll.documentView = table
        view = scroll
    }

    func update(sections: [(group: String, sessions: [SidebarSession])], selected: Int?) {
        suppressSelection = true
        defer { suppressSelection = false }
        self.selected = selected
        rows = sections.flatMap { section -> [Row] in
            [.group(section.group, section.sessions.count)]
                + section.sessions.map(Row.session)
        }
        table.reloadData()
        if let selected, let index = rows.firstIndex(where: {
            if case let .session(session) = $0 { return session.id == selected }
            return false
        }) {
            table.selectRowIndexes([index], byExtendingSelection: false)
        }
    }

    private func session(at row: Int) -> SidebarSession? {
        guard rows.indices.contains(row), case let .session(session) = rows[row] else { return nil }
        return session
    }

    private func group(at row: Int) -> String? {
        guard rows.indices.contains(row), case let .group(name, _) = rows[row] else { return nil }
        return name
    }

    // MARK: - table

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool {
        if case .group = rows[row] { return true }
        return false
    }

    func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        if case .group = rows[row] { return false }
        return true
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch rows[row] {
        case let .group(name, count):
            let label = rowLabel("\(name)  \(count)", size: 11, bold: true)
            label.textColor = .secondaryLabelColor
            let view = NSTableCellView()
            view.addSubview(label)
            label.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                label.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 2),
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
        for subview in [dot, title, subtitle] {
            view.addSubview(subview)
            subview.translatesAutoresizingMaskIntoConstraints = false
        }
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
        guard !suppressSelection, let session = session(at: table.selectedRow) else { return }
        onSelect?(session.id)
    }

    // MARK: - context menu

    private func buildMenu() -> NSMenu {
        let menu = NSMenu()
        menu.addItem(withTitle: "Close Session", action: #selector(closeClicked), keyEquivalent: "")
        menu.addItem(withTitle: "Mark as Read", action: #selector(markReadClicked), keyEquivalent: "")
        menu.addItem(withTitle: "Rename Session…", action: #selector(renameClicked), keyEquivalent: "")
        menu.addItem(withTitle: "Rename Group…", action: #selector(renameGroupClicked), keyEquivalent: "")
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
        prompt(title: "Rename session", current: session.title) { [weak self] name in
            self?.onRename?(session.id, name)
        }
    }

    @objc private func renameGroupClicked() {
        guard let name = group(at: table.clickedRow) else { return }
        prompt(title: "Rename group", current: name) { [weak self] newName in
            self?.onRenameGroup?(name, newName)
        }
    }

    private func prompt(title: String, current: String, apply: @escaping (String) -> Void) {
        let alert = NSAlert()
        alert.messageText = title
        let field = NSTextField(string: current)
        field.frame = NSRect(x: 0, y: 0, width: 240, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if !name.isEmpty { apply(name) }
    }

    // MARK: - drag to reorder

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        guard let session = session(at: row) else { return nil }
        let item = NSPasteboardItem()
        item.setString(String(session.id), forType: .string)
        return item
    }

    func tableView(
        _ tableView: NSTableView,
        validateDrop info: NSDraggingInfo,
        proposedRow row: Int,
        proposedDropOperation op: NSTableView.DropOperation
    ) -> NSDragOperation {
        .move
    }

    func tableView(
        _ tableView: NSTableView,
        acceptDrop info: NSDraggingInfo,
        row: Int,
        dropOperation op: NSTableView.DropOperation
    ) -> Bool {
        guard let text = info.draggingPasteboard.string(forType: .string), let id = Int(text) else {
            return false
        }
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
