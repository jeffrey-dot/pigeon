import Foundation
import PigeonCore

/// How this window reaches a remote machine (`docs/remote.md` §1).
///
/// The three ways are probed from best to worst, and the window always says which one it got:
/// - `proxy`: `ssh host pigeon-host --proxy` — the host runs on the far side, sessions outlive
///   the window.
/// - `forward`: `ssh -L <local sock>:<remote sock>` — same host, a different way in (needs
///   `AllowStreamLocalForwarding yes`).
/// - `degraded`: plain `ssh -tt host` — no persistence at all, but the program still runs and
///   `OSC 7501` status still arrives; this window keeps the VT state.
final class RemoteSession {
    enum Transport: String, Equatable {
        case proxy
        case forward
        case degraded
    }

    struct Outcome {
        var transport: Transport
        /// A protocol client for `proxy`/`forward`; `nil` when degraded.
        var client: HostClient?
        /// The ssh process a degraded session talks through (its stdin/stdout are the pty).
        var process: Process?
        /// One line for the user: which way in, and why not the better one.
        var note: String
    }

    let host: String
    private let installer: HostInstaller
    private let run: (String) throws -> HostInstaller.Result
    private let spawn: ([String]) throws -> Process
    /// Finds the host build to push for a remote's `uname -sm` (tests pass their own, so the
    /// outcome does not depend on what happens to be in `core/target`).
    private let artifact: (String) -> URL?
    private var children: [Process] = []

    init(
        host: String,
        installer: HostInstaller = HostInstaller(),
        run: ((String) throws -> HostInstaller.Result)? = nil,
        spawn: (([String]) throws -> Process)? = nil,
        artifact: ((String) -> URL?)? = nil
    ) {
        self.host = host
        self.installer = installer
        self.artifact = artifact ?? { platform in
            HostInstaller.artifact(for: platform, in: HostInstaller.searchDirectories())
        }
        self.run = run ?? { script in
            try HostInstaller.sshRunner(host: host, script: script, stdin: nil)
        }
        self.spawn = spawn ?? { arguments in
            try RemoteSession.ssh(arguments)
        }
    }

    /// The probe order as data, so the rule is testable without a network.
    static func choose(proxy: Bool, forward: Bool) -> Transport {
        if proxy { return .proxy }
        if forward { return .forward }
        return .degraded
    }

    /// Tries every transport and returns the first that answers.
    ///
    /// `allowDegraded: false` is the reconnect path: falling back to plain ssh there would
    /// silently replace a persistent session with one that dies with the window, so a
    /// failed probe has to stay a failure the caller can retry.
    func connect(
        allowInstall: Bool = true,
        allowHost: Bool = true,
        allowDegraded: Bool = true
    ) throws -> Outcome {
        var why: [String] = []
        if allowHost {
            do {
                let remote = try installer.install(
                    host: host, artifact: try installArtifact(), allowInstall: allowInstall)
                try startDaemon(binary: remote.path)
                if let client = try probeProxy(binary: remote.path) {
                    return Outcome(
                        transport: .proxy,
                        client: client,
                        note: "ssh bridge to \(host) — sessions keep running there")
                }
                why.append("--proxy did not answer")
                let socket = try remoteSocket(binary: remote.path)
                if let client = try probeForward(socket: socket) {
                    return Outcome(
                        transport: .forward,
                        client: client,
                        note: "ssh -L to \(host) — sessions keep running there")
                }
                why.append("ssh -L was refused or did not open \(socket)")
            } catch {
                why.append("\(error)")
            }
        } else {
            why.append("host binary disabled")
        }
        guard allowDegraded else {
            throw HostInstaller.Failure.noHost(why.joined(separator: "; "))
        }
        let process = try spawn(["-tt", host])
        children.append(process)
        return Outcome(
            transport: .degraded,
            client: nil,
            process: process,
            note: "\(host) has no usable pigeon-host (\(why.joined(separator: "; "))) — "
                + "this session ends with the window")
    }

    /// A host to push: the one for the remote's own platform, if we have it built.
    private func installArtifact() throws -> URL {
        let probe = try run("uname -sm")
        let platform = probe.output.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let artifact = artifact(platform) else {
            throw HostInstaller.Failure.noArtifact(platform)
        }
        return artifact
    }

    /// Starts (or reuses) the remote host in the background; `nohup` is what lets it outlive
    /// the ssh channel that started it.
    func startDaemon(binary: String) throws {
        let script = """
            if [ ! -x "\(binary)" ]; then exit 4; fi
            if "\(binary)" --socket-path >/dev/null 2>&1; then :; fi
            if pgrep -f "\(binary)" >/dev/null 2>&1; then exit 0; fi
            nohup "\(binary)" </dev/null >/dev/null 2>&1 &
            sleep 0.3
            """
        let result = try run(script)
        if !result.ok {
            throw HostInstaller.Failure.command(
                result.error.isEmpty ? result.output : result.error)
        }
    }

    /// The remote socket path, asked of the binary itself so the rule lives in one place.
    func remoteSocket(binary: String) throws -> String {
        let result = try run("\"\(binary)\" --socket-path")
        guard result.ok else {
            throw HostInstaller.Failure.command(result.error)
        }
        return result.output.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func probeProxy(binary: String) throws -> HostClient? {
        let process = try spawn([host, "\"\(binary)\" --proxy"])
        guard let ends = Self.pipeEnds(process) else {
            process.terminate()
            return nil
        }
        let client = HostClient(read: ends.output.fileDescriptor, write: ends.input.fileDescriptor)
        if handshake(client) {
            children.append(process)
            return client
        }
        client.close()
        process.terminate()
        return nil
    }

    private func probeForward(socket: String) throws -> HostClient? {
        let local = NSTemporaryDirectory() + "pigeon-remote-\(UUID().uuidString.prefix(8)).sock"
        try? FileManager.default.removeItem(atPath: local)
        let process = try spawn(["-N", "-L", "\(local):\(socket)", host])
        defer {
            // The classic trick: keep the forward alive only as long as it is useful.
            if !children.contains(where: { $0 === process }) { process.terminate() }
        }
        let deadline = Date().addingTimeInterval(3)
        while Date() < deadline {
            if FileManager.default.fileExists(atPath: local),
               let client = try? HostClient.connect(path: local),
               handshake(client)
            {
                children.append(process)
                return client
            }
            if !process.isRunning { return nil }
            usleep(100_000)
        }
        return nil
    }

    /// A cheap "is this really a pigeon host" question; the protocol makes it safe to ask.
    private func handshake(_ client: HostClient) -> Bool {
        let probe = HandshakeProbe()
        // The callbacks run on the client's read queue and may fire after the wait timed
        // out, so they hold the probe (a reference), never stack or freed memory.
        client.onMessage = { message in
            defer { Wire.free(message) }
            if Wire.type(message) == PG_MSG_HELLO_ACK { probe.finish(answered: true) }
        }
        client.onClose = { probe.finish(answered: false) }
        guard let hello = Wire.make(PG_MSG_HELLO) else { return false }
        Wire.set(hello, u32: "proto", 0x0100)
        Wire.set(hello, str: "client", "pigeon-mac")
        Wire.set(hello, strings: "features", ["scrollback", "delta", "status_all"])
        client.send(hello)
        Wire.free(hello)
        let answered = probe.wait(seconds: 5)
        client.onMessage = nil
        client.onClose = nil
        return answered
    }

    func close() {
        for child in children where child.isRunning {
            child.terminate()
        }
        children.removeAll()
    }

    /// The two ends of a child's stdio, whether they are pipes or handed to us as handles.
    static func pipeEnds(_ process: Process) -> (input: FileHandle, output: FileHandle)? {
        if let input = process.standardInput as? Pipe,
           let output = process.standardOutput as? Pipe {
            return (input.fileHandleForWriting, output.fileHandleForReading)
        }
        if let input = process.standardInput as? FileHandle,
           let output = process.standardOutput as? FileHandle {
            return (input, output)
        }
        return nil
    }

    static func ssh(_ arguments: [String]) throws -> Process {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
        // A forward has to stay owned by its own process (see `sshOptions`).
        let forwards = arguments.contains("-L")
        process.arguments = HostInstaller.sshOptions(multiplex: !forwards) + arguments
        let stdin = Pipe()
        let stdout = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = FileHandle.nullDevice
        try process.run()
        return process
    }
}
