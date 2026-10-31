import Foundation

/// What the sidebar remembers between launches. Lives in `~/.config/pigeon/`
/// (`AGENTS.md` forbids `~/Library` for app state).
struct SidebarState: Codable, Equatable {
    var collapsed: [String] = []
    /// Manual row order as `SessionRef.key`s (`"3"`, `"devbox:2"`).
    var order: [String] = []
    /// Session id (as text, JSON keys are strings) → user title.
    var renamed: [String: String] = [:]
    /// Session id → unread count, so a reopen keeps the dots.
    var unread: [String: Int] = [:]
    /// Project group key → user-visible name.
    var renamedGroups: [String: String] = [:]

    var collapsedSet: Set<String> { Set(collapsed) }

    init() {}

    private enum CodingKeys: String, CodingKey {
        case collapsed, order, renamed, unread, renamedGroups
    }

    /// Tolerant: a missing key keeps its default, and an `order` older builds wrote (packed
    /// Int ids, `host index * 100_000 + session`) is mapped back to keys for this Mac and
    /// dropped for remote hosts, whose index could have meant any machine.
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        collapsed = (try? container.decodeIfPresent([String].self, forKey: .collapsed)) ?? []
        renamed = (try? container.decodeIfPresent([String: String].self, forKey: .renamed)) ?? [:]
        unread = (try? container.decodeIfPresent([String: Int].self, forKey: .unread)) ?? [:]
        renamedGroups = (try? container.decodeIfPresent([String: String].self, forKey: .renamedGroups)) ?? [:]
        if let keys = try? container.decodeIfPresent([String].self, forKey: .order) {
            order = keys
        } else if let packed = try? container.decodeIfPresent([Int].self, forKey: .order) {
            order = packed.compactMap { id in
                id / 100_000 == 1 && id % 100_000 > 0 ? String(id % 100_000) : nil
            }
        }
    }
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
