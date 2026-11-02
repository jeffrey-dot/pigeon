import CryptoKit
import Foundation

/// Finds, downloads and installs a newer Pigeon.app from GitHub Releases (`docs/update.md`).
///
/// Every call blocks; `UpdateController` runs them off the main thread. Nothing is trusted
/// until it has passed the release key's signature and the archive hash it vouches for.
public struct Updater {
    public enum Failure: Error, CustomStringConvertible {
        case network(String)
        /// Manifest malformed or unsigned, archive hash or bundle not what was signed.
        case untrusted(String)
        case cannotReplace(String)

        /// Shown to the user as is.
        public var description: String {
            switch self {
            case .network:
                return "The download didn't finish. Make sure you're online, then try again."
            case .untrusted:
                return "The update didn't pass Pigeon's safety check, so it wasn't installed."
            case let .cannotReplace(why):
                return why
            }
        }
    }

    public static let manifestURL = URL(
        string: "https://github.com/jeffrey-dot/pigeon/releases/latest/download/latest.json")!
    /// Public half of the release signing key. CI signs with the private half
    /// (`PIGEON_UPDATE_KEY`); a key change means every installed copy has to be updated by hand.
    public static let publicKeyBase64 = "DEdlLrrmHNUh2CVZoTakhGWcOxk4psdBOY9U8anFEBQ="
    /// The archive holds exactly this bundle.
    static let appName = "Pigeon.app"

    public let current: String
    let manifestURL: URL
    let publicKey: Curve25519.Signing.PublicKey
    let bundleIdentifier: String
    let fetch: ServerDownload.Fetch

    public init(
        current: String,
        bundleIdentifier: String,
        publicKey: Curve25519.Signing.PublicKey,
        manifestURL: URL = Updater.manifestURL,
        fetch: @escaping ServerDownload.Fetch = ServerDownload.httpGet
    ) {
        self.current = current
        self.bundleIdentifier = bundleIdentifier
        self.publicKey = publicKey
        self.manifestURL = manifestURL
        self.fetch = fetch
    }

    /// The updater for this copy of the app, or `nil` for a build without a release version
    /// (`swift run`, tests): those are updated the way they were built.
    public static func standard(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        info: [String: Any]? = Bundle.main.infoDictionary
    ) -> Updater? {
        guard let version = info?["CFBundleShortVersionString"] as? String,
              AppVersion(version) != nil,
              let identifier = info?["CFBundleIdentifier"] as? String,
              let raw = Data(base64Encoded: publicKeyBase64),
              let key = try? Curve25519.Signing.PublicKey(rawRepresentation: raw)
        else { return nil }
        let url = environment["PIGEON_UPDATE_URL"].flatMap { $0.isEmpty ? nil : URL(string: $0) }
        return Updater(
            current: version, bundleIdentifier: identifier, publicKey: key,
            manifestURL: url ?? manifestURL)
    }

    /// The newer release, or `nil` when this copy is the latest.
    public func check() throws -> UpdateManifest? {
        let data: Data
        do {
            data = try fetch(manifestURL)
        } catch {
            throw Failure.network("\(error)")
        }
        guard let manifest = UpdateManifest.parse(data) else {
            throw Failure.untrusted("malformed latest.json")
        }
        guard manifest.isSigned(by: publicKey) else {
            throw Failure.untrusted("latest.json is not signed by the release key")
        }
        guard let offered = AppVersion(manifest.version), let running = AppVersion(current),
              offered > running
        else { return nil }
        return manifest
    }

    /// Downloads and unpacks `manifest`'s archive. The new app lands in a scratch directory
    /// on the same volume as `destination`, so `install` is a rename, not a copy.
    public func download(_ manifest: UpdateManifest, near destination: URL) throws -> URL {
        let archive: Data
        do {
            archive = try fetch(manifest.url)
        } catch {
            throw Failure.network("\(error)")
        }
        guard ServerDownload.sha256(archive) == manifest.sha256 else {
            throw Failure.untrusted("archive does not match the signed hash")
        }
        let manager = FileManager.default
        let scratch = try manager.url(
            for: .itemReplacementDirectory, in: .userDomainMask,
            appropriateFor: destination, create: true)
        let zip = scratch.appendingPathComponent("update.zip")
        try archive.write(to: zip)
        defer { try? manager.removeItem(at: zip) }
        try Self.unzip(zip, into: scratch)
        let app = scratch.appendingPathComponent(Self.appName)
        try verify(app: app, version: manifest.version)
        return app
    }

    /// The unpacked bundle is this app, at the version that was signed, and can start.
    func verify(app: URL, version: String) throws {
        let contents = app.appendingPathComponent("Contents")
        guard let info = NSDictionary(contentsOf: contents.appendingPathComponent("Info.plist")),
              info["CFBundleIdentifier"] as? String == bundleIdentifier,
              info["CFBundleShortVersionString"] as? String == version,
              let executable = info["CFBundleExecutable"] as? String,
              FileManager.default.isExecutableFile(
                  atPath: contents.appendingPathComponent("MacOS/\(executable)").path)
        else { throw Failure.untrusted("the archive does not hold Pigeon \(version)") }
    }

    /// Puts `app` where `destination` is. The running process keeps its open files, so it
    /// carries on unchanged until it relaunches.
    public static func install(_ app: URL, replacing destination: URL) throws {
        // macOS runs a quarantined app from a read-only copy; replacing that copy changes
        // nothing the user will open next time.
        if destination.path.contains("/AppTranslocation/") {
            throw Failure.cannotReplace(
                "Move Pigeon to your Applications folder, open it from there, then try again.")
        }
        let folder = destination.deletingLastPathComponent()
        guard FileManager.default.isWritableFile(atPath: folder.path) else {
            throw Failure.cannotReplace(
                "Pigeon can't make changes in \(folder.path). "
                    + "Move Pigeon to a folder you can write to, such as Applications, then try again.")
        }
        do {
            _ = try FileManager.default.replaceItemAt(destination, withItemAt: app)
        } catch {
            throw Failure.cannotReplace("Pigeon couldn't replace itself: \(error.localizedDescription)")
        }
        try? FileManager.default.removeItem(at: app.deletingLastPathComponent())
    }

    /// Opens `app` once process `pid` has exited, so the new copy starts after this one quits.
    public static func relaunch(_ app: URL, after pid: Int32) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = [
            "-c", "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"$2\"",
            "sh", String(pid), app.path,
        ]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
    }

    /// `ditto` is how macOS itself unpacks app archives: it keeps the bundle's symlinks,
    /// permissions and code signature intact.
    static func unzip(_ zip: URL, into directory: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        process.arguments = ["-x", "-k", zip.path, directory.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw Failure.untrusted("the archive could not be unpacked")
        }
    }
}
