import Foundation

/// Terminal preferences that outlive a launch. Lives in `~/.config/pigeon/settings.json`
/// (`AGENTS.md` forbids `~/Library` for app state).
struct TerminalSettings: Codable, Equatable {
    /// ⌥ sends `ESC` + key (what readline's `M-b`/`M-f` expect) instead of composing
    /// characters like `é` or, on many European layouts, `@` and `[`.
    var optionAsMeta = true

    init(optionAsMeta: Bool = true) {
        self.optionAsMeta = optionAsMeta
    }

    /// Missing keys keep their defaults, so a file written by an older build still loads.
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        optionAsMeta = try container.decodeIfPresent(Bool.self, forKey: .optionAsMeta) ?? true
    }
}

final class TerminalSettingsStore {
    private let url: URL

    init(url: URL = TerminalSettingsStore.defaultURL) {
        self.url = url
    }

    static var defaultURL: URL {
        if let override = ProcessInfo.processInfo.environment["PIGEON_CONFIG_DIR"] {
            return URL(fileURLWithPath: override).appendingPathComponent("settings.json")
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config/pigeon/settings.json")
    }

    func load() -> TerminalSettings {
        guard let data = try? Data(contentsOf: url),
              let settings = try? JSONDecoder().decode(TerminalSettings.self, from: data)
        else {
            return TerminalSettings()
        }
        return settings
    }

    /// Atomic write; a failure to save must never disturb the UI.
    func save(_ settings: TerminalSettings) {
        guard let data = try? JSONEncoder().encode(settings) else { return }
        let directory = url.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temporary = directory.appendingPathComponent(".settings-\(UUID().uuidString).json")
        guard (try? data.write(to: temporary, options: .atomic)) != nil else { return }
        _ = try? FileManager.default.replaceItemAt(url, withItemAt: temporary)
    }
}
