import Foundation

/// The machines this window knows about (`docs/remote.md` §4). One window is one connection,
/// so this is the list you switch between, not a set of live sockets.
struct HostsState: Codable, Equatable {
    /// Hosts the user has added, in the order they appear in the menu.
    var hosts: [String] = []
    /// The host that was connected last; `nil` means this Mac.
    var current: String?
}

final class HostsStore {
    private let url: URL

    init(url: URL = HostsStore.defaultURL) {
        self.url = url
    }

    static var defaultURL: URL {
        if let override = ProcessInfo.processInfo.environment["PIGEON_CONFIG_DIR"] {
            return URL(fileURLWithPath: override).appendingPathComponent("hosts.json")
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config/pigeon/hosts.json")
    }

    /// Entries ssh cannot use (`ssh asia`, typed as a command) are repaired or dropped here,
    /// so a bad entry saved once does not fail every launch.
    func load() -> HostsState {
        guard let data = try? Data(contentsOf: url),
              var state = try? JSONDecoder().decode(HostsState.self, from: data)
        else {
            return HostsState()
        }
        state.hosts = HostName.cleaned(state.hosts)
        state.current = state.current.flatMap { try? HostName.parse($0).get() }
        return state
    }

    /// Atomic write; a failure to save must never disturb the UI.
    func save(_ state: HostsState) {
        guard let data = try? JSONEncoder().encode(state) else { return }
        let directory = url.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temporary = directory.appendingPathComponent(".hosts-\(UUID().uuidString).json")
        guard (try? data.write(to: temporary, options: .atomic)) != nil else { return }
        _ = try? FileManager.default.replaceItemAt(url, withItemAt: temporary)
    }
}
