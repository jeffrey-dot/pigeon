import Foundation
import PigeonCore

/// How this window reaches a remote machine (`docs/remote.md` §1).
///
/// The three ways are probed from best to worst, and the window always says which one it got:
/// - `proxy`: `ssh host pigeon-server --proxy` — the host runs on the far side, sessions outlive
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
        /// One line for the user: what this connection can and cannot do.
        var note: String
        /// Why the better transports failed, for the log (never shown: it is plumbing).
        var detail: String = ""
    }

    let host: String
    private let installer: HostInstaller
    private let run: (String) throws -> HostInstaller.Result
    private let spawn: ([String]) throws -> Process
    /// Finds the server build to push for a remote's `uname -sm`: a local build if there is
    /// one (development), else this version's release download. Tests pass their own, so the
    /// outcome does not depend on what happens to be in `core/target` or on the network.
    private let artifact: (String) throws -> URL?
    private var children: [Process] = []

    init(
        host: String,
        installer: HostInstaller = HostInstaller(),
        run: ((String) throws -> HostInstaller.Result)? = nil,
        spawn: (([String]) throws -> Process)? = nil,
        artifact: ((String) throws -> URL?)? = nil
    ) {
        self.host = host
        self.installer = installer
        self.artifact = artifact ?? { platform in
            if let local = HostInstaller.artifact(for: platform, in: HostInstaller.searchDirectories()) {
                return local
            }
            return try ServerDownload.standard()?.binary(for: platform)
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
        // Before any ssh runs, and not a reason to degrade: plain ssh would take it too.
        try HostInstaller.checkDestination(host)
        var why: [String] = []
        if allowHost {
            do {
                let remote = try installer.install(
                    host: host, allowInstall: allowInstall, artifact: localArtifact)
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
            } catch let failure as HostInstaller.Failure {
                // Plain ssh cannot reach a machine the probe could not: say so, not "no host".
                if case .unreachable = failure { throw failure }
                if case .invalidDestination = failure { throw failure }
                why.append("\(failure)")
            } catch {
                why.append("\(error)")
            }
        } else {
            why.append("host binary disabled")
        }
        guard allowDegraded else {
            throw HostInstaller.Failure.noHost(why.joined(separator: "; "))
        }
        let process = try spawn(["-tt", "--", host])
        children.append(process)
        return Outcome(
            transport: .degraded,
            client: nil,
            process: process,
            note: "Couldn't set up Pigeon on \(host), so this is a plain SSH session. "
                + "It ends when you close the window.",
            detail: why.joined(separator: "; "))
    }

    /// The build to push to a remote whose `pigeon-server` is missing or out of date.
    private func localArtifact(platform: String) throws -> URL {
        guard let artifact = try artifact(platform) else {
            throw HostInstaller.Failure.noArtifact(platform)
        }
        return artifact
    }

    /// Starts (or reuses) the remote host the way the local one is started: `--detach` asks
    /// the socket whether a host answers, starts one in its own
    /// process group otherwise, and returns once it is listening.
    ///
    /// A host already serving the socket is kept, whatever its version: replacing it would end
    /// the user's sessions there. A major-version mismatch therefore surfaces as a refused
    /// `hello` (`docs/remote.md` §3), not as a silent restart.
    func startDaemon(binary: String) throws {
        let quoted = HostInstaller.shellQuote(binary)
        let script = """
            if [ ! -x \(quoted) ]; then exit 4; fi
            exec \(quoted) --detach </dev/null >/dev/null
            """
        let result = try run(script)
        if !result.ok {
            throw HostInstaller.Failure.command(
                result.error.isEmpty ? result.output : result.error)
        }
    }

    /// The remote socket path, asked of the binary itself so the rule lives in one place.
    func remoteSocket(binary: String) throws -> String {
        let result = try run("\(HostInstaller.shellQuote(binary)) --socket-path")
        guard result.ok else {
            throw HostInstaller.Failure.command(result.error)
        }
        return result.output.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func probeProxy(binary: String) throws -> HostClient? {
        let process = try spawn(["--", host, "\(HostInstaller.shellQuote(binary)) --proxy"])
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
        let process = try spawn(["-N", "-L", "\(local):\(socket)", "--", host])
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
        Wire.set(hello, u32: "proto", UInt32(pg_proto_version()))
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
