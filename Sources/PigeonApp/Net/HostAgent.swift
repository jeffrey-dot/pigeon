import Foundation

/// Runs the local `pigeon-host` as a launchd agent, so it outlives the app: quitting or
/// crashing the window must not take the sessions with it (issue #2). The app only writes
/// the plist and asks launchd; it never parents the host itself.
///
/// The job is `KeepAlive` on abnormal exit only: a crashed host comes back, while
/// `stop()` (`launchctl bootout`) unloads the job and really ends every session.
public struct HostAgent {
    public typealias Runner = (_ arguments: [String]) -> (status: Int32, output: String)

    public static let defaultLabel = "dev.pigeon.host"

    public let label: String
    /// Where the plist lives; launchd reloads `~/Library/LaunchAgents` at login.
    public let plistURL: URL
    public let logURL: URL
    let uid: uid_t
    let launchctl: Runner

    public init(
        label: String = HostAgent.defaultLabel,
        plistURL: URL? = nil,
        logURL: URL? = nil,
        uid: uid_t = getuid(),
        launchctl: @escaping Runner = HostAgent.runLaunchctl
    ) {
        let home = FileManager.default.homeDirectoryForCurrentUser
        self.label = label
        self.plistURL = plistURL
            ?? home.appendingPathComponent("Library/LaunchAgents/\(label).plist")
        // Next to the rest of pigeon's state (`AGENTS.md`: `~/.config/pigeon`).
        self.logURL = logURL ?? home.appendingPathComponent(".config/pigeon/host.log")
        self.uid = uid
        self.launchctl = launchctl
    }

    var domain: String { "gui/\(uid)" }
    var service: String { "\(domain)/\(label)" }

    /// The job description. The socket and shell are passed explicitly: launchd starts the
    /// host with its own minimal environment, which would otherwise pick another socket
    /// path than the app looks at (`XDG_RUNTIME_DIR`) or fall back to `/bin/sh`.
    public func plist(hostPath: String, socket: String, shell: String?, environment: [String: String])
        throws -> Data
    {
        var arguments = [hostPath, "--socket", socket]
        if let shell, !shell.isEmpty {
            arguments += ["--shell", shell]
        }
        var job: [String: Any] = [
            "Label": label,
            "ProgramArguments": arguments,
            "RunAtLoad": true,
            "KeepAlive": ["SuccessfulExit": false],
            "ProcessType": "Interactive",
            "StandardOutPath": logURL.path,
            "StandardErrorPath": logURL.path,
        ]
        // What the shells inherit; without PATH/LANG a launchd job sees `/usr/bin:/bin` and C.
        let kept = environment.filter { Self.inherited.contains($0.key) && !$0.value.isEmpty }
        if !kept.isEmpty {
            job["EnvironmentVariables"] = kept
        }
        return try PropertyListSerialization.data(fromPropertyList: job, format: .xml, options: 0)
    }

    static let inherited: Set<String> = ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "XDG_RUNTIME_DIR"]

    /// Installs (or refreshes) the plist and asks launchd to start the host now. `nil` on
    /// success, otherwise what launchctl said.
    public func start(hostPath: String, socket: String, shell: String?,
                      environment: [String: String] = ProcessInfo.processInfo.environment) -> String?
    {
        let data: Data
        do {
            data = try plist(hostPath: hostPath, socket: socket, shell: shell, environment: environment)
        } catch {
            return "cannot describe the host job: \(error)"
        }
        let manager = FileManager.default
        try? manager.createDirectory(
            at: logURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        if (try? Data(contentsOf: plistURL)) != data {
            // A loaded job keeps its old arguments until it is unloaded; this only runs when
            // the socket is unreachable, so no live host is being cut off.
            _ = launchctl(["bootout", service])
            do {
                try manager.createDirectory(
                    at: plistURL.deletingLastPathComponent(), withIntermediateDirectories: true)
                try data.write(to: plistURL, options: .atomic)
            } catch {
                return "cannot write \(plistURL.path): \(error)"
            }
        }
        // Fails harmlessly when the job is already loaded; kickstart is what must succeed.
        _ = launchctl(["bootstrap", domain, plistURL.path])
        let kick = launchctl(["kickstart", service])
        guard kick.status == 0 else {
            return "launchctl kickstart \(service) failed (\(kick.status)): \(kick.output)"
        }
        return nil
    }

    /// Unloads the job, which terminates the host and with it every session. The plist is
    /// removed too, or the host would come back at the next login.
    public func stop() -> String? {
        let result = launchctl(["bootout", service])
        try? FileManager.default.removeItem(at: plistURL)
        guard result.status == 0 else {
            return "launchctl bootout \(service) failed (\(result.status)): \(result.output)"
        }
        return nil
    }

    /// Whether launchd knows the job (loaded, running or not).
    public func isLoaded() -> Bool {
        launchctl(["print", service]).status == 0
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
