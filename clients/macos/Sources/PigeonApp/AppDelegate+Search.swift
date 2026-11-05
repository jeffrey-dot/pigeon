import AppKit
import PigeonCore

extension AppDelegate {
    func installSearch(in content: NSView) {
        searchBar = SearchBarView(frame: .zero)
        searchBar.isHidden = true
        content.addSubview(searchBar)
        searchBar.translatesAutoresizingMaskIntoConstraints = false
        searchHeight = searchBar.heightAnchor.constraint(equalToConstant: 0)
        NSLayoutConstraint.activate([
            searchBar.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            searchBar.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            searchBar.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            container.bottomAnchor.constraint(equalTo: searchBar.topAnchor), searchHeight,
        ])
        searchBar.onChange = { [weak self] pattern, sensitive in self?.beginSearch(pattern, sensitive: sensitive) }
        searchBar.onRestart = { [weak self] in
            guard let self else { return }
            beginSearch(searchBar.pattern, sensitive: searchBar.caseSensitive)
        }
        searchBar.onNavigate = { [weak self] step in self?.navigateSearch(step) }
        searchBar.onStop = { [weak self] in
            self?.searchModel?.cancel()
            self?.refreshSearch()
        }
        container.onCancelSearch = { [weak self] in
            guard let self, searchModel != nil else { return false }
            closeSearch()
            return true
        }
        searchBar.onClose = { [weak self] in self?.closeSearch() }
    }

    var searchContext: TerminalSearch.Context? {
        guard connection.ready, localSession == nil, let pane = focusedPane,
              displayedHost == activeHost, displayedSession == selectedSession,
              container.paneView(pane) != nil, let instance = connection.instance else { return nil }
        return .init(host: activeHost, instance: instance, connection: connection.generation, pane: pane)
    }

    @objc func findInPane(_ sender: Any?) {
        guard let context = searchContext else { return }
        let opening = searchModel?.context != context
        if opening {
            closeSearch(focus: false)
            // Ordinary scrollback suppresses live frames at the host. Search needs those
            // notifications to mark a completed scan expired when the program writes again.
            sendScroll(context.pane, lines: 0)
            searchModel = TerminalSearch(context: context)
        }
        searchBar.isHidden = false
        searchHeight.constant = 64
        window.contentView?.layoutSubtreeIfNeeded()
        if connection.textSearch {
            searchBar.enable()
            refreshSearch()
            searchBar.focus()
            if opening, !searchBar.pattern.isEmpty { beginSearch(searchBar.pattern, sensitive: searchBar.caseSensitive) }
        } else { searchBar.unavailable() }
    }

    func beginSearch(_ pattern: String, sensitive: Bool) {
        guard searchModel?.context == searchContext, connection.textSearch else { return }
        searchModel?.begin(pattern: pattern, caseSensitive: sensitive)
        clearSearchHighlight()
        refreshSearch()
        // Coalesce typing without retaining any server task. The token also covers close/reopen.
        let token = UUID()
        searchDebounce = token
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            guard let self, searchDebounce == token else { return }
            requestSearchPage()
        }
    }

    private func requestSearchPage() {
        guard var model = searchModel, model.context == searchContext,
              model.scanning, model.pageRequest == nil, let client else { return }
        let id = requestID()
        guard let message = SearchWire.request(id: id, model: model) else {
            searchModel?.cancel(); refreshSearch(); return
        }
        defer { pg_msg_free(message) }
        model.requestPage(id: id)
        searchModel = model
        client.send(message)
    }

    func navigateSearch(_ step: Int) {
        guard var model = searchModel, model.context == searchContext, let client,
              let range = model.select(step: step) else { return }
        let id = requestID()
        guard let message = SearchWire.request(id: id, model: model, range: range) else { return }
        defer { pg_msg_free(message) }
        model.requestLocation(id: id)
        searchModel = model
        clearSearchHighlight()
        refreshSearch()
        client.send(message)
    }

    /// Called before the normal protocol handler, with the actual source host.
    func handleSearch(_ message: OpaquePointer, host: String) -> Bool {
        guard var model = searchModel, model.context.host == host,
              model.context == searchContext else { return false }
        let type = Wire.type(message)
        if type == PG_MSG_ERROR {
            guard model.fail(id: Wire.u32(message, "id"), context: model.context,
                             code: Wire.string(message, "code")) else { return false }
            searchModel = model; clearSearchHighlight(); refreshSearch()
            return true
        }
        if type == PG_MSG_TEXT_QUERY_RESULT {
            guard let result = pg_msg_text_result(message) else { return true }
            defer { pg_text_result_free(result) }
            guard let info = SearchWire.info(result), info.pane_id == model.context.pane else { return true }
            if let page = SearchWire.page(result) {
                guard model.receivePage(id: page.id, context: model.context, generation: page.generation,
                                        hits: page.hits, next: page.next) else { return true }
                let selectFirst = model.selected == nil && !model.hits.isEmpty
                searchModel = model
                refreshSearch()
                if selectFirst { navigateSearch(1) }
                // Yield between pages so Stop/edit/close can cancel even over a local socket.
                DispatchQueue.main.async { [weak self] in self?.requestSearchPage() }
            } else if info.kind == 3, let range = SearchWire.range(result, index: 0),
                      model.receiveLocation(id: info.id, context: model.context, generation: info.generation, range: range),
                      let viewport = SearchWire.viewport(result),
                      let view = container.paneView(info.pane_id), let snapshot = pg_text_result_snapshot(result) {
                defer { pg_snapshot_free(snapshot) }
                searchModel = model
                // The regular delta baseline no longer describes the visible picture.
                haveSnapshot.remove(info.pane_id)
                connection.receivedScreen.remove(info.pane_id)
                frameSeq[info.pane_id] = nil
                pendingSnapshots[info.pane_id] = nil
                view.apply(snapshot: snapshot)
                view.searchFrozen = true
                view.modes = viewport.modes
                view.clearSelection()
                view.setScrollPosition(total: Int(viewport.history_len), offset: Int(viewport.offset))
                view.searchHighlight = range.spans(topRow: min(range.start.row, 0), cols: view.cols,
                                                   rows: view.rows, widths: view.cells.map(\.width))
                view.screenChanged()
                refreshSearch()
                refreshStatusPanel()
            }
            return true
        }
        if type == PG_MSG_SNAPSHOT || type == PG_MSG_DELTA,
           Wire.u32(message, "pane_id") == model.context.pane {
            // A located picture is immutable even at offset=0. Never patch it with a live delta.
            let frozen = model.frozen
            let modes = Wire.u32(message, "modes", fallback: UInt32.max)
            if modes != UInt32.max { container.paneView(model.context.pane)?.modes = modes }
            if model.generation != 0 { model.expire(); searchModel = model; refreshSearch() }
            return frozen
        }
        return false
    }

    func refreshSearch() {
        if let model = searchModel { searchBar.update(model) }
    }
    func clearSearchHighlight() {
        guard let pane = searchModel?.context.pane, let view = container.paneView(pane) else { return }
        view.searchHighlight = []
        view.screenChanged()
    }
    func closeSearch(focus: Bool = true) {
        guard let model = searchModel else { return }
        clearSearchHighlight()
        container.paneView(model.context.pane)?.searchFrozen = false
        searchModel = nil
        searchDebounce = UUID()
        searchBar.isHidden = true
        searchHeight.constant = 0
        // Scroll zero is the existing live-screen recovery path; no historical seq is reused.
        if connections[model.context.host]?.generation == model.context.connection,
           connections[model.context.host]?.ready == true {
            onHost(model.context.host) { sendScroll(model.context.pane, lines: 0) }
        }
        if focus, displayedHost == model.context.host { container.focusPane(model.context.pane) }
    }
}
