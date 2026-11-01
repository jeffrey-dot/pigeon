import CryptoKit
import Foundation

/// What a release says about itself: `latest.json`, published next to the app archive by
/// `.github/workflows/release.yml` (`docs/update.md`).
///
/// The signature covers the version and the archive's hash together, so nobody can pair a
/// genuine archive with another version number, or a version number with another archive.
public struct UpdateManifest: Equatable, Decodable {
    public var version: String
    public var url: URL
    /// SHA-256 of the archive at `url`, lowercase hex.
    public var sha256: String
    /// Ed25519 over `signedMessage`, base64.
    public var signature: String

    /// The exact bytes the release key signs; CI builds the same string with `printf`.
    public var signedMessage: Data {
        Data("pigeon-update \(version) \(sha256)".utf8)
    }

    /// A manifest that is complete and well formed, or `nil`. Nothing is half-read: a bad
    /// version, hash or non-https URL rejects the whole file.
    public static func parse(_ data: Data) -> UpdateManifest? {
        guard let manifest = try? JSONDecoder().decode(UpdateManifest.self, from: data),
              AppVersion(manifest.version) != nil,
              manifest.sha256.count == 64,
              manifest.sha256.unicodeScalars.allSatisfy({
                  ("0"..."9").contains($0) || ("a"..."f").contains($0)
              }),
              manifest.url.scheme == "https"
        else { return nil }
        return manifest
    }

    public func isSigned(by key: Curve25519.Signing.PublicKey) -> Bool {
        guard let raw = Data(base64Encoded: signature) else { return false }
        return key.isValidSignature(raw, for: signedMessage)
    }
}
