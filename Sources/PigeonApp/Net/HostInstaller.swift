import CryptoKit
import Foundation

/// Pushes `pigeon-server` to a remote machine over the system `ssh` (`docs/remote.md` §2).
///
/// Rules this file exists to keep: no package manager and no root; everything lives under
/// `~/.pigeon/`; the version directory is the binary's content hash, so "already installed"
/// is an exact answer; a transfer is verified *on the remote* before it becomes live.
public struct HostInstaller {
    public struct Remote: Equatable {
        /// `uname -sm`, e.g. `Linux x86_64`.
        public var platform: String
        /// Content-hash prefix: the directory name under `~/.pigeon/server/`.
        public var version: String
        /// Absolute path of the installed binary on the remote.
        public var path: String
        /// `false` when a matching binary was already there and nothing was sent.
        public var sent: Bool
    }

    public struct Result {
        public var status: Int32
        public var output: String
        /// Where the shell put its complaints; also where the remote hash mismatch lands.
        public var error: String
        public var ok: Bool { status == 0 }
        /// ssh exits 255 for its own errors (DNS, refused, closed, auth); a remote command's
        /// failure has any other code.
        public var unreachable: Bool { status == 255 }
        /// ssh's last word on why: its final non-empty stderr line.
        public var reason: String {
            error.split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .last { !$0.isEmpty } ?? ""
        }
    }

    public enum Failure: Error, CustomStringConvertible {
        case command(String)
        case unsupportedPlatform(String)
        case noArtifact(String)
        case rejected(String)
        /// The remote could not be reached as a `pigeon-server` at all (degraded mode refused).
        case noHost(String)
        /// ssh itself failed (exit 255): the machine was never reached, so no fallback can help.
        case unreachable(host: String, detail: String)

        public var description: String {
            switch self {
            case let .command(detail): return "ssh: \(detail)"
            case let .unsupportedPlatform(platform): return "no pigeon-server build for \(platform)"
            case let .noArtifact(platform): return "no local pigeon-server binary for \(platform)"
            case let .rejected(detail): return "remote refused the install: \(detail)"
            // The detail is plumbing (missing builds, refused forwards): logged, not shown.
            case .noHost: return "Couldn't set up Pigeon on the server"
            case let .unreachable(host, detail):
                return detail.isEmpty ? "Can't connect to \(host)" : "Can't connect to \(host): \(detail)"
            }
        }
    }

    /// `uname -sm` values we can serve → the Rust target triple built for them (the release
    /// asset names, `.github/workflows/release.yml`). Linux builds are static musl, so one
    /// per CPU covers every distribution.
    public static let platforms: [String: String] = [
        "Linux x86_64": "x86_64-unknown-linux-musl",
        "Linux i686": "i686-unknown-linux-musl",
        "Linux i386": "i686-unknown-linux-musl",
        "Linux aarch64": "aarch64-unknown-linux-musl",
        "Linux arm64": "aarch64-unknown-linux-musl",
        // 32-bit ARM with hardware floats: Raspberry Pi 2+ / most boards (v7), Pi 1 / Zero (v6).
        "Linux armv7l": "armv7-unknown-linux-musleabihf",
        "Linux armv8l": "armv7-unknown-linux-musleabihf",
        "Linux armv6l": "arm-unknown-linux-musleabihf",
        "Linux riscv64": "riscv64gc-unknown-linux-musl",
        "Darwin arm64": "aarch64-apple-darwin",
        "Darwin x86_64": "x86_64-apple-darwin",
    ]

    /// The triple built for a `uname -sm`, if any.
    public static func triple(for platform: String) -> String? {
        platforms[platform]
    }

    /// Every triple a release carries, once each.
    public static var triples: [String] {
        Set(platforms.values).sorted()
    }

    /// Runs a shell command on `host`; `stdin` is the file to stream into it. Injected so the
    /// rules can be tested without a network.
    public typealias Runner = (_ host: String, _ script: String, _ stdin: URL?) throws -> Result
    public typealias Hash = (_ file: URL) throws -> String

    private let run: Runner
    private let hash: Hash

    public init(
        run: @escaping Runner = HostInstaller.sshRunner,
        hash: @escaping Hash = HostInstaller.contentHash
    ) {
        self.run = run
        self.hash = hash
    }

    /// What the remote machine is, as `uname` sees it.
    public func platform(of host: String) throws -> String {
        let result = try run(host, "uname -sm", nil)
        guard result.ok else {
            throw Failure.command(result.error.isEmpty ? result.output : result.error)
        }
        return result.output.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Installs `artifact` on `host` unless an identical binary is already there.
    ///
    /// `allowInstall` is the "first connection asks once" switch: with it off, a host that
    /// has no matching binary is an error rather than a silent upload.
    public func install(host: String, artifact: URL, allowInstall: Bool = true) throws -> Remote {
        let platform = try platform(of: host)
        guard Self.triple(for: platform) != nil else {
            throw Failure.unsupportedPlatform(platform)
        }
        let full = try hash(artifact)
        let version = String(full.prefix(16))
        let directory = "$HOME/.pigeon/server/\(version)"
        let path = "\(directory)/pigeon-server"

        // Cheap first question: is this exact content already there?
        let probe = try run(host, Self.remoteHash(path), nil)
        guard probe.ok else {
            throw Failure.command(probe.error.isEmpty ? probe.output : probe.error)
        }
        if Self.firstWord(probe.output) == full {
            let resolved = try run(host, "printf '%s' \"\(path)\"", nil)
            return Remote(
                platform: platform,
                version: version,
                path: resolved.output,
                sent: false
            )
        }
        guard allowInstall else {
            throw Failure.rejected("install the host once with PIGEON_HOST_ARTIFACTS or enable it")
        }

        let script = Self.installScript(directory: directory, expected: full)
        do {
            let result = try run(host, script, artifact)
            guard result.ok else {
                // The remote deletes the partial file itself; all we do is say why.
                throw Failure.rejected(
                    result.error.isEmpty ? result.output : result.error
                )
            }
            return Remote(
                platform: platform,
                version: version,
                path: result.output.trimmingCharacters(in: .whitespacesAndNewlines),
                sent: true
            )
        } catch let failure as Failure {
            throw failure
        } catch {
            throw Failure.command("\(error)")
        }
    }

    // MARK: - pieces that are worth testing on their own

    /// The shell run on the remote: write a temp file next to the target, verify the hash
    /// **there** (a cut-off transfer must never become live), move it in, keep two versions.
    ///
    /// The temp file comes from `mktemp` (same flags on Linux and macOS): two windows
    /// installing at once each verify and rename their own file, so neither can truncate the
    /// other's between the hash check and the `mv`. No `trap`: the runner already owns `EXIT`
    /// (it removes its helper script), so every failure path cleans up by hand.
    public static func installScript(directory: String, expected: String) -> String {
        """
        set -e
        mkdir -p "\(directory)"
        tmp=$(mktemp "\(directory)/.pigeon-server.XXXXXX")
        fail() {
            rm -f "$tmp"
            # A rejected transfer leaves nothing behind, not even its directory.
            rmdir "\(directory)" 2>/dev/null || true
            echo "pigeon-server: $1" >&2
            exit "$2"
        }
        cat > "$tmp" || fail "transfer failed" 4
        chmod 755 "$tmp" || fail "chmod failed" 4
        if command -v sha256sum >/dev/null 2>&1; then
            got=$(sha256sum "$tmp" | cut -d' ' -f1) || fail "sha256sum failed" 4
        else
            got=$(shasum -a 256 "$tmp" | cut -d' ' -f1) || fail "shasum failed" 4
        fi
        if [ "$got" != "\(expected)" ]; then
            fail "hash mismatch (got $got)" 3
        fi
        mv -f "$tmp" "\(directory)/pigeon-server" || fail "mv failed" 4
        # Newest two survive; older runs keep their own binary until the session restarts.
        ls -1t "$HOME/.pigeon/server" 2>/dev/null | tail -n +3 | while read -r old; do
            rm -rf "$HOME/.pigeon/server/$old"
        done
        # Builds before the rename lived in ~/.pigeon/host. A host still running from there
        # keeps working (the socket is the same); only its file goes.
        rm -rf "$HOME/.pigeon/host"
        printf '%s' "\(directory)/pigeon-server"
        """
    }

    /// Prints the remote file's sha256, or nothing when it is not there.
    public static func remoteHash(_ path: String) -> String {
        """
        if [ -f "\(path)" ]; then
            if command -v sha256sum >/dev/null 2>&1; then
                sha256sum "\(path)"
            else
                shasum -a 256 "\(path)"
            fi
        fi
        """
    }

    public static func firstWord(_ text: String) -> String {
        text.split(whereSeparator: \.isWhitespace).first.map(String.init) ?? ""
    }

    /// SHA-256 of a file, as lowercase hex (what `sha256sum` prints).
    public static func contentHash(_ file: URL) throws -> String {
        let data = try Data(contentsOf: file)
        return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    /// The local binary that runs on `platform`, if we have one.
    public static func artifact(for platform: String, in directories: [URL]) -> URL? {
        guard let triple = triple(for: platform) else { return nil }
        for directory in directories {
            let named = directory.appendingPathComponent("pigeon-server-\(triple)")
            if FileManager.default.isExecutableFile(atPath: named.path) {
                return named
            }
            // cargo keeps the triple in the path (`target/<triple>/release`) and the
            // binary under its plain name.
            if directory.pathComponents.contains(triple) {
                let plain = directory.appendingPathComponent("pigeon-server")
                if FileManager.default.isExecutableFile(atPath: plain.path) {
                    return plain
                }
            }
        }
        return nil
    }

    /// Where a local build can be, most specific first. Release builds of the app ship no
    /// remote binaries; they download them (`ServerDownload`).
    public static func searchDirectories(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        workingDirectory: String = FileManager.default.currentDirectoryPath
    ) -> [URL] {
        var directories: [URL] = []
        if let explicit = environment["PIGEON_HOST_ARTIFACTS"], !explicit.isEmpty {
            directories.append(URL(fileURLWithPath: explicit))
        }
        for triple in triples {
            directories.append(
                URL(fileURLWithPath: workingDirectory)
                    .appendingPathComponent("core/target/\(triple)/release"))
        }
        directories.append(
            URL(fileURLWithPath: workingDirectory).appendingPathComponent("core/target/release"))
        return directories
    }

    /// Options every pigeon ssh call shares. Connecting takes six or seven separate ssh runs
    /// (probe, hash, start, bridge…), and each one used to pay the full handshake (~0.6 s to
    /// a far host). With a shared master connection only the first does; the rest reuse it
    /// (~0.1 s). Nothing is written to the user's ssh config: these are command-line options.
    ///
    /// `multiplex: false` is for `ssh -N -L`: through a master the forward moves into the
    /// master and the command exits at once, so its process could no longer say whether the
    /// forward is up, nor take it down when terminated.
    public static func sshOptions(multiplex: Bool = true, uid: uid_t = getuid()) -> [String] {
        var options = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=15"]
        guard multiplex, let directory = controlDirectory(uid: uid) else { return options }
        options += [
            "-o", "ControlMaster=auto",
            // `%C` is a hash of host/port/user, so one socket per destination and a short path
            // (unix socket paths are capped at ~104 bytes).
            "-o", "ControlPath=\(directory)/%C",
            "-o", "ControlPersist=120",
        ]
        return options
    }

    /// `/tmp/pigeon-<uid>-ssh`, private to this user; `nil` (no multiplexing) when it cannot
    /// be made safe — someone else's directory there must never carry our ssh sessions.
    static func controlDirectory(uid: uid_t) -> String? {
        let path = "/tmp/pigeon-\(uid)-ssh"
        let manager = FileManager.default
        try? manager.createDirectory(
            atPath: path, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        guard let attributes = try? manager.attributesOfItem(atPath: path),
              attributes[.type] as? FileAttributeType == .typeDirectory,
              (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == uid,
              (attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700
        else { return nil }
        return path
    }

    /// The real transport: `ssh -o BatchMode=yes HOST SCRIPT`, stdin from `stdin`.
    ///
    /// The remote's login shell is whatever the user runs — bash, zsh, fish, csh — and ssh
    /// hands it our command line to parse. So the command line stays a flat pipeline all of
    /// those understand, and the real script travels as base64 on it: written under
    /// `~/.pigeon/` and run with `/bin/sh`. That also keeps stdin free for the binary.
    public static func sshRunner(host: String, script: String, stdin: URL?) throws -> Result {
        // Whatever the outcome, the helper file goes away: the only things left under
        // `~/.pigeon/` are the host binaries.
        let body = "trap 'rm -f \"$0\"' EXIT\n" + script
        let encoded = Data(body.utf8).base64EncodedString()
        let pipeline = "mkdir -p \"$HOME/.pigeon\"; echo \(encoded) | base64 -d "
            + "> \"$HOME/.pigeon/install.sh\"; sh \"$HOME/.pigeon/install.sh\""
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
        process.arguments = sshOptions() + [host, pipeline]
        let out = Pipe()
        let err = Pipe()
        process.standardOutput = out
        process.standardError = err
        if let stdin {
            process.standardInput = try FileHandle(forReadingFrom: stdin)
        } else {
            process.standardInput = FileHandle.nullDevice
        }
        try process.run()
        // Read before waiting: a chatty child would otherwise block on a full pipe.
        let output = out.fileHandleForReading.readDataToEndOfFile()
        let error = err.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return Result(
            status: process.terminationStatus,
            output: String(decoding: output, as: UTF8.self),
            error: String(decoding: error, as: UTF8.self)
        )
    }
}
