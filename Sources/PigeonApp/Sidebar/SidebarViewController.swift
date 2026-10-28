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
    private var headerControlsCenter: NSLayoutConstraint!
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
    var onNewSession: ((String) -> Void)?
    var onToggleSidebar: (() -> Void)?

    override func loadView() {
        let root = SidebarBackgroundView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        table.headerView = nil
        table.rowHeight = SidebarMetrics.rowHeight
        table.intercellSpacing = NSSize(width: 0, height: SidebarMetrics.rowGap)
        table.selectionHighlightStyle = .none
        table.style = .plain
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
        buttons.spacing = SidebarMetrics.headerButtonSpacing

        // §2.3 empty state: one line of copy plus a big primary button.
        let hint = rowLabel("No sessions yet", size: SidebarMetrics.needsLabelSize, bold: false)
        hint.textColor = SidebarTheme.gray
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
        scroll.contentInsets = NSEdgeInsets(top: SidebarMetrics.listTopPadding, left: 0, bottom: 0, right: 0)
        // The traffic lights sit in the titlebar overlay, so the header row is centered on
        // them by the app once the window exists (`alignHeaderControls(centerFromTop:)`).
        headerControlsCenter = buttons.centerYAnchor.constraint(
            equalTo: root.topAnchor, constant: SidebarMetrics.headerHeight / 2)
        NSLayoutConstraint.activate([
            buttons.heightAnchor.constraint(equalToConstant: SidebarMetrics.headerButtonSize),
            headerControlsCenter,
            buttons.trailingAnchor.constraint(
                equalTo: root.trailingAnchor, constant: -SidebarMetrics.headerPadRight),
            // The header is a fixed 56pt band (harp's HEADER_HEIGHT); the chips sit on the
            // traffic-light row inside it, and the list always starts below the band.
            scroll.topAnchor.constraint(
                equalTo: root.topAnchor, constant: SidebarMetrics.headerHeight),
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
        let config = NSImage.SymbolConfiguration(
            pointSize: SidebarMetrics.headerButtonIconSize, weight: .regular)
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
        let button = SidebarHeaderButton(image: image ?? NSImage(), target: self, action: action)
        button.isBordered = false
        button.contentTintColor = SidebarTheme.gray
        button.widthAnchor.constraint(equalToConstant: SidebarMetrics.headerButtonSize).isActive = true
        button.heightAnchor.constraint(equalToConstant: SidebarMetrics.headerButtonSize).isActive = true
        return button
    }

    /// Lines the header chips up with the traffic lights instead of the 56pt header's middle.
    func alignHeaderControls(centerFromTop: CGFloat) {
        headerControlsCenter?.constant = centerFromTop
    }

    @objc private func toggleSidebarClicked() { onToggleSidebar?() }

    @objc private func newSessionClicked() {
        if ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil {
            FileHandle.standardError.write(Data("pigeon: + clicked\n".utf8))
        }
        onNewSession?("")
    }

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

    /// A group header reserves the gap above it, the way harp spaces its sections.
    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        if case .group = rows[row] {
            return SidebarMetrics.sectionHeaderHeight + SidebarMetrics.sectionGap
        }
        return SidebarMetrics.rowHeight
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        SidebarRowView()
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch rows[row] {
        case let .group(name, count, needs):
            let cwd = rows.first { row in
                if case let .session(session) = row, session.group == name { return true }
                return false
            }
            .flatMap { row -> String? in
                if case let .session(session) = row { return session.cwd }
                return nil
            } ?? ""
            return groupCell(name: name, count: count, needs: needs, cwd: cwd)
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

    private func groupCell(name: String, count: Int, needs: Int, cwd: String) -> NSView {
        let view = SidebarGroupCellView()
        view.configure(group: name, count: count, needs: needs)
        view.onNewSession = { [weak self] in self?.onNewSession?(cwd) }
        return view
    }

    private func sessionCell(_ session: SidebarSession) -> NSView {
        let view = SidebarRowCellView()
        view.configure(with: session)
        view.onClose = { [weak self] in self?.onClose?(session.id) }
        return view
    }

    /// Status colors from harp's token table (`docs/ui.md` §7.4).
    /// PIGEON_DEBUG only: the drag path is invisible otherwise, and the GUI check script
    /// prints these lines as evidence.
    private func debugLog(_ text: String) {
        guard ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil else { return }
        FileHandle.standardError.write(Data("pigeon: \(text)\n".utf8))
    }

    private func statusColor(_ session: SidebarSession) -> NSColor {
        switch session.status {
        case .blocked: return SidebarTheme.iconNeedsRing
        case .error: return SidebarTheme.error
        case .done: return session.unread > 0 ? SidebarTheme.iconStrong : SidebarTheme.ring
        case .working: return SidebarTheme.iconStrong
        case .idle: return SidebarTheme.ring
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
        debugLog("drag start row=\(row) id=\(session.id)")
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
        debugLog("drop row=\(row) target=\(target.map(String.init) ?? "-") order=\(ids)")
        onReorder?(ids)
        return true
    }
}
