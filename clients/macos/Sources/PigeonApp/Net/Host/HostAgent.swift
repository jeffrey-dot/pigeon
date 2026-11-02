import Foundation

/// Removes the launchd job older builds registered (`dev.pigeon.host`). The host is now
/// started like a `tmux` server (`HostLauncher.detachArguments`), and a registered job is
/// what made macOS list pigeon-host under "Allow in the Background".
public struct HostAgent {
    public typealias Runner = (_ arguments: [String]) -> (status: Int32, output: String)

    public static let defaultLabel = "dev.pigeon.host"

    public let label: String
    public let plistURL: URL
    let uid: uid_t
    let launchctl: Runner

    public init(
        label: String = HostAgent.defaultLabel,
        plistURL: URL? = nil,
        uid: uid_t = getuid(),
        launchctl: @escaping Runner = HostAgent.runLaunchctl
    ) {
        self.label = label
        self.plistURL = plistURL ?? FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/\(label).plist")
        self.uid = uid
        self.launchctl = launchctl
    }

    var service: String { "gui/\(uid)/\(label)" }

    /// Whether an old job is still on disk. Cheap (no launchctl), so it runs at every launch.
    public var isInstalled: Bool {
        FileManager.default.fileExists(atPath: plistURL.path)
    }

    /// Deletes the old plist, so the job neither runs at the next login nor shows up as a
    /// background item. A job whose host is running is left loaded: unloading it would end
    /// that host and every session in it, and it goes away at logout anyway. A job with no
    /// running host is unloaded too, or launchd would keep relaunching it while the new host
    /// owns the socket. Runs only while the plist exists, i.e. once per Mac.
    @discardableResult
    public func removeLegacyJob() -> Bool {
        guard isInstalled else { return false }
        let running = Self.pid(inPrint: launchctl(["print", service]).output) != nil
        if !running { _ = launchctl(["bootout", service]) }
        try? FileManager.default.removeItem(at: plistURL)
        return true
    }

    /// The `pid = N` line of `launchctl print`; nil when the job is loaded but not running.
    static func pid(inPrint output: String) -> Int32? {
        for line in output.split(separator: "\n") {
            let parts = line.split(separator: "=", maxSplits: 1)
            guard parts.count == 2, parts[0].trimmingCharacters(in: .whitespaces) == "pid" else { continue }
            if let pid = Int32(parts[1].trimmingCharacters(in: .whitespaces)), pid > 0 { return pid }
        }
        return nil
    }

    public static func runLaunchctl(_ arguments: [String]) -> (status: Int32, output: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        process.arguments = arguments
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        process.standardInput = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return (-1, "\(error)")
        }
        let output = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, String(decoding: output, as: UTF8.self))
    }
}
