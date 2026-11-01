import CryptoKit
import Foundation

/// Fetches the `pigeon-server` build for a remote's platform from this version's GitHub
/// release, the first time a machine of that platform is connected (`docs/remote.md` §2).
/// The app carries no remote builds: each one is downloaded once, checked against the
/// release's `SHA256SUMS`, and cached under `~/.cache/pigeon/server/<version>/`.
public struct ServerDownload {
    public typealias Fetch = (_ url: URL) throws -> Data
    public typealias Unzip = (_ gzip: Data) throws -> Data

    public enum Failure: Error, CustomStringConvertible {
        case unknownPlatform(String)
        case notListed(String)
        case corrupt(String)
        case network(String)
        case insecure(String)

        public var description: String {
            switch self {
            case let .unknownPlatform(platform): return "no pigeon-server build for \(platform)"
            case let .notListed(name): return "\(name) is not in the release's SHA256SUMS"
            case let .corrupt(name): return "\(name) does not match its SHA256SUMS entry"
            case let .network(detail): return "download failed: \(detail)"
            case let .insecure(url): return "refusing to download from \(url): it must use https"
            }
        }
    }

    /// Where releases live; `PIGEON_SERVER_URL` points at a mirror instead.
    public static let releases = "https://github.com/jeffrey-dot/pigeon/releases/download"

    public let version: String
    let base: URL
    let cache: URL
    let fetch: Fetch
    let unzip: Unzip

    public init(
        version: String,
        base: URL? = nil,
        cache: URL? = nil,
        fetch: @escaping Fetch = ServerDownload.httpGet,
        unzip: @escaping Unzip = ServerDownload.gunzip
    ) {
        self.version = version
        self.base = base ?? URL(string: "\(Self.releases)/v\(version)")!
        self.cache = cache ?? FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".cache/pigeon/server")
        self.fetch = fetch
        self.unzip = unzip
    }

    /// The download for this build of the app, or `nil` for a development build that has no
    /// bundle version (it uses the binaries cargo left in `core/target`).
    public static func standard(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        bundleVersion: String? = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String
    ) -> ServerDownload? {
        guard let version = environment["PIGEON_SERVER_VERSION"] ?? bundleVersion,
              !version.isEmpty
        else { return nil }
        let base = environment["PIGEON_SERVER_URL"].flatMap { $0.isEmpty ? nil : URL(string: $0) }
        return ServerDownload(version: version, base: base)
    }

    /// Release asset name for a Rust target triple.
    public static func assetName(triple: String) -> String {
        "pigeon-server-\(triple).gz"
    }

    /// The binary for `platform` (`uname -sm`), downloading it on first use.
    public func binary(for platform: String) throws -> URL {
        guard let triple = HostInstaller.triple(for: platform) else {
            throw Failure.unknownPlatform(platform)
        }
        let directory = cache.appendingPathComponent(version)
        let target = directory.appendingPathComponent("pigeon-server-\(triple)")
        // Only a verified download is ever renamed into place, so a cached file is trusted.
        if FileManager.default.isExecutableFile(atPath: target.path) { return target }

        // `SHA256SUMS` comes from the same origin as the binary, so it only catches a broken
        // download; against tampering the transport is all there is. Releases are not signed
        // yet (`docs/remote.md` §5).
        guard Self.isTrusted(base) else { throw Failure.insecure(base.absoluteString) }
        let name = Self.assetName(triple: triple)
        let sums = Self.parseSums(String(decoding: try fetch(base.appendingPathComponent("SHA256SUMS")), as: UTF8.self))
        guard let expected = sums[name] else { throw Failure.notListed(name) }
        let compressed = try fetch(base.appendingPathComponent(name))
        guard Self.sha256(compressed) == expected else { throw Failure.corrupt(name) }
        let binary = try unzip(compressed)

        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temporary = directory.appendingPathComponent(".\(UUID().uuidString).tmp")
        defer { try? FileManager.default.removeItem(at: temporary) }
        try binary.write(to: temporary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: temporary.path)
        if FileManager.default.fileExists(atPath: target.path) {
            try FileManager.default.removeItem(at: target)
        }
        try FileManager.default.moveItem(at: temporary, to: target)
        return target
    }

    /// https anywhere; plain http only to this machine (a local mirror for testing), where
    /// there is no network path to tamper with.
    public static func isTrusted(_ url: URL) -> Bool {
        switch url.scheme?.lowercased() {
        case "https": return true
        case "http": return ["localhost", "127.0.0.1", "::1"].contains(url.host?.lowercased() ?? "")
        default: return false
        }
    }

    /// `sha256sum` output (`<hex>  <name>`, or `<hex> *<name>` in binary mode) → name: hex.
    /// Lines that are not exactly that are skipped, never half-read.
    public static func parseSums(_ text: String) -> [String: String] {
        var sums: [String: String] = [:]
        for line in text.split(whereSeparator: \.isNewline) {
            let fields = line.split(whereSeparator: \.isWhitespace)
            guard fields.count == 2 else { continue }
            let hash = fields[0].lowercased()
            guard hash.count == 64, hash.allSatisfy(\.isHexDigit) else { continue }
            var name = String(fields[1])
            if name.hasPrefix("*") { name.removeFirst() }
            sums[name] = hash
        }
        return sums
    }

    static func sha256(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    /// A blocking GET; callers are already off the main thread (`RemoteSession.connect`).
    public static func httpGet(_ url: URL) throws -> Data {
        final class Box: @unchecked Sendable {
            var result: Result<Data, Error> = .failure(Failure.network("timed out"))
        }
        let box = Box()
        let done = DispatchSemaphore(value: 0)
        var request = URLRequest(url: url, timeoutInterval: 60)
        request.setValue("pigeon", forHTTPHeaderField: "User-Agent")
        URLSession.shared.dataTask(with: request) { data, response, error in
            defer { done.signal() }
            if let error {
                box.result = .failure(Failure.network(error.localizedDescription))
                return
            }
            // A redirect could leave https (GitHub sends assets to another host); what was
            // actually read must have come over a trusted transport too.
            if let final = response?.url, !ServerDownload.isTrusted(final) {
                box.result = .failure(Failure.insecure(final.absoluteString))
                return
            }
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            guard status == 200, let data else {
                box.result = .failure(Failure.network("HTTP \(status) for \(url.lastPathComponent)"))
                return
            }
            box.result = .success(data)
        }.resume()
        _ = done.wait(timeout: .now() + 90)
        return try box.result.get()
    }

    /// Foundation has no gzip reader; the system `gzip` does it.
    public static func gunzip(_ compressed: Data) throws -> Data {
        let input = FileManager.default.temporaryDirectory
            .appendingPathComponent("pigeon-server-\(UUID().uuidString).gz")
        try compressed.write(to: input)
        defer { try? FileManager.default.removeItem(at: input) }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/gzip")
        process.arguments = ["-dc", input.path]
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        try process.run()
        let data = out.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0, !data.isEmpty else {
            throw Failure.corrupt("gzip stream")
        }
        return data
    }
}
