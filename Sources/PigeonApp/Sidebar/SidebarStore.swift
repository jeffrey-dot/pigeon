import Foundation

/// What the sidebar remembers between launches. Lives in `~/.config/pigeon/`
/// (`AGENTS.md` forbids `~/Library` for app state).
struct SidebarState: Codable, Equatable {
    var collapsed: [String] = []
    var order: [Int] = []
    /// Session id (as text, JSON keys are strings) → user title.
    var renamed: [String: String] = [:]
    /// Session id → unread count, so a reopen keeps the dots.
    var unread: [String: Int] = [:]

    var collapsedSet: Set<String> { Set(collapsed) }
}

final class SidebarStore {
    private let url: URL

    init(url: URL = SidebarStore.defaultURL) {
        self.url = url
    }

    static var defaultURL: URL {
        // `PIGEON_CONFIG_DIR` lets tests (and portable setups) keep state out of
        // the real `~/.config/pigeon`.
        if let override = ProcessInfo.processInfo.environment["PIGEON_CONFIG_DIR"] {
            return URL(fileURLWithPath: override).appendingPathComponent("sidebar.json")
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config/pigeon/sidebar.json")
    }

    func load() -> SidebarState {
        guard let data = try? Data(contentsOf: url),
              let state = try? JSONDecoder().decode(SidebarState.self, from: data)
        else {
            return SidebarState()
        }
        return state
    }

    /// Atomic write; a failure to save must never disturb the UI.
    func save(_ state: SidebarState) {
        guard let data = try? JSONEncoder().encode(state) else { return }
        let directory = url.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temporary = directory.appendingPathComponent(".sidebar-\(UUID().uuidString).json")
        guard (try? data.write(to: temporary, options: .atomic)) != nil else { return }
        _ = try? FileManager.default.replaceItemAt(url, withItemAt: temporary)
    }
}
