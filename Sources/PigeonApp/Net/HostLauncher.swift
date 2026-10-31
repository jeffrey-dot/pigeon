import Foundation

/// Starts `pigeon-host` when the socket is missing. A terminal window that needs the user to
/// open a shell and start a daemon by hand is not a terminal yet.
public enum HostLauncher {
    /// Places a locally built host can be, most specific first.
    ///
    /// shortcut: a fixed candidate list (env override, next to the app, cargo target dirs).
    /// M4's remote installer will add `~/.pigeon/host/<version>/`; revisit then.
    public static func candidates(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        executable: String = CommandLine.arguments.first ?? "",
        workingDirectory: String = FileManager.default.currentDirectoryPath
    ) -> [String] {
        var paths: [String] = []
        if let explicit = environment["PIGEON_HOST_PATH"], !explicit.isEmpty {
            paths.append(explicit)
        }
        if !executable.isEmpty {
            let directory = (executable as NSString).deletingLastPathComponent
            paths.append((directory as NSString).appendingPathComponent("pigeon-host"))
        }
        for target in ["debug", "release"] {
            paths.append(
                (workingDirectory as NSString)
                    .appendingPathComponent("core/target/\(target)/pigeon-host"))
        }
        return paths
    }

    /// The first candidate that is actually runnable.
    public static func find(candidates: [String]) -> String? {
        candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    /// The arguments that start a host the way `tmux` starts its server: `--detach` puts it
    /// in its own process group and returns once it answers, so the window quitting, crashing
    /// or being killed never takes the sessions along. Nothing is registered with the system
    /// (no launchd job, no login item): the host lives until "End All Sessions on This Mac…" or logout.
    public static func detachArguments(socket: String, shell: String?, log: String) -> [String] {
        var arguments = ["--detach", "--socket", socket, "--log", log]
        if let shell, !shell.isEmpty { arguments += ["--shell", shell] }
        return arguments
    }

    /// Runs `path` with `arguments` in the background; `false` when it cannot be spawned.
    @discardableResult
    public static func launch(path: String, arguments: [String] = []) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return false
        }
        return true
    }
}
