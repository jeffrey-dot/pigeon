import AppKit

/// The session list: project groups as headers, one row per session. Rows are
/// derived data (grouping, order, copy) from `SidebarModel`; this class only
/// draws them and reports user intent.
final class SidebarViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private enum Row {
        case group(String, Int, Int)
        case session(SidebarSession)
    }

    private let table = NSTableView()
    private let scroll = NSScrollView()
    private let emptyState = NSStackView()
    private var rows: [Row] = []
    private var ticker: Timer?
    private var selected: Int?
    /// Programmatic reloads must not look like the user clicking a row.
    private var suppressSelection = false

    var onSelect: ((Int) -> Void)?
    var onClose: ((Int) -> Void)?
    var onMarkRead: ((Int) -> Void)?
    var onRename: ((Int, String) -> Void)?
    var onRenameGroup: ((String, String) -> Void)?
    var onReorder: (([Int]) -> Void)?
    var onNewSession: (() -> Void)?
    var onToggleSidebar: (() -> Void)?

    override func loadView() {
        let root = NSView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        table.headerView = nil
        table.rowHeight = 26
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

        // `docs/ui.md` §2.3: collapse and new-session buttons, right-aligned.
        let buttons = NSStackView(views: [
            symbolButton("sidebar.left", action: #selector(toggleSidebarClicked)),
            symbolButton("plus", action: #selector(newSessionClicked)),
        ])
        buttons.orientation = .horizontal
        buttons.spacing = 2

        // §2.3 empty state: one line of copy plus a big primary button.
        let hint = rowLabel("No sessions yet", size: 12, bold: false)
        hint.textColor = .secondaryLabelColor
        let create = NSButton(title: "New Session", target: self, action: #selector(newSessionClicked))
        create.bezelStyle = .rounded
        emptyState.orientation = .vertical
        emptyState.spacing = 10
        emptyState.alignment = .centerX
        emptyState.addArrangedSubview(hint)
        emptyState.addArrangedSubview(create)
        emptyState.isHidden = true

        for subview in [buttons, scroll, emptyState] {
            root.addSubview(subview)
            subview.translatesAutoresizingMaskIntoConstraints = false
        }
        NSLayoutConstraint.activate([
            buttons.topAnchor.constraint(equalTo: root.topAnchor, constant: 6),
            buttons.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -8),
            buttons.heightAnchor.constraint(equalToConstant: 20),
            scroll.topAnchor.constraint(equalTo: buttons.bottomAnchor, constant: 4),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            emptyState.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            emptyState.centerYAnchor.constraint(equalTo: root.centerYAnchor),
        ])
        view = root
        // Ages go stale without any data change; the reference refreshes once a minute.
        ticker = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in
            self?.table.reloadData()
        }
    }

    deinit {
        ticker?.invalidate()
    }

    private func symbolButton(_ symbol: String, action: Selector) -> NSButton {
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
        let button = NSButton(image: image ?? NSImage(), target: self, action: action)
        button.isBordered = false
        button.contentTintColor = .secondaryLabelColor
        return button
    }

    @objc private func toggleSidebarClicked() { onToggleSidebar?() }

    @objc private func newSessionClicked() { onNewSession?() }

    func update(sections: [(group: String, sessions: [SidebarSession])], selected: Int?) {
        suppressSelection = true
        defer { suppressSelection = false }
        self.selected = selected
        rows = sections.flatMap { section -> [Row] in
            [.group(
                section.group,
                section.sessions.count,
                SidebarModel.needsYou(section.sessions)
            )] + section.sessions.map(Row.session)
        }
        table.reloadData()
        scroll.isHidden = rows.isEmpty
        emptyState.isHidden = !rows.isEmpty
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
        guard rows.indices.contains(row), case let .group(name, _, _) = rows[row] else { return nil }
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
        case let .group(name, count, needs):
            return groupCell(name: name, count: count, needs: needs)
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

    private func groupCell(name: String, count: Int, needs: Int) -> NSView {
        let view = NSTableCellView()
        let title = rowLabel("\(name)  \(count)", size: 11, bold: true)
        title.textColor = .secondaryLabelColor
        view.addSubview(title)
        title.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 6),
            title.centerYAnchor.constraint(equalTo: view.centerYAnchor),
        ])
        // `1 needs you` only when a row in this group wants the user.
        if needs > 0 {
            let badge = rowLabel("\(needs) needs you", size: 11, bold: false)
            badge.textColor = .systemOrange
            view.addSubview(badge)
            badge.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                badge.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -8),
                badge.centerYAnchor.constraint(equalTo: view.centerYAnchor),
                badge.leadingAnchor.constraint(greaterThanOrEqualTo: title.trailingAnchor, constant: 6),
            ])
        }
        return view
    }

    /// One line per the reference: title left; `<age> <glyph>` in a right-hand column.
    private func sessionCell(_ session: SidebarSession) -> NSView {
        let view = NSTableCellView()
        let unread = session.unread > 0

        let glyph = rowLabel(SidebarModel.statusGlyph(for: session), size: 11, bold: false)
        glyph.textColor = statusColor(session)
        glyph.alignment = .center
        let age = rowLabel(SidebarModel.ageText(for: session), size: 11, bold: false)
        age.textColor = unread ? .labelColor : .secondaryLabelColor
        age.alignment = .right
        let title = rowLabel(session.title, size: 13, bold: unread)
        view.addSubview(glyph)
        view.addSubview(age)
        view.addSubview(title)

        var leading = view.leadingAnchor
        var inset: CGFloat = 10
        if unread {
            let dot = NSView()
            dot.wantsLayer = true
            dot.layer?.cornerRadius = 2.5
            dot.layer?.backgroundColor = NSColor.controlAccentColor.cgColor
            view.addSubview(dot)
            dot.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                dot.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 4),
                dot.centerYAnchor.constraint(equalTo: view.centerYAnchor),
                dot.widthAnchor.constraint(equalToConstant: 5),
                dot.heightAnchor.constraint(equalToConstant: 5),
            ])
            leading = dot.trailingAnchor
            inset = 5
        }

        for label in [glyph, age, title] { label.translatesAutoresizingMaskIntoConstraints = false }
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leading, constant: inset),
            title.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            title.trailingAnchor.constraint(lessThanOrEqualTo: age.leadingAnchor, constant: -6),
            glyph.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -8),
            glyph.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            glyph.widthAnchor.constraint(equalToConstant: 12),
            age.trailingAnchor.constraint(equalTo: glyph.leadingAnchor, constant: -6),
            age.centerYAnchor.constraint(equalTo: view.centerYAnchor),
        ])
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
