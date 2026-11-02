import Foundation

/// Resolves a repository-relative path from a test's `#filePath`.
///
/// Tests sit at `clients/macos/Tests/PigeonAppTests/`, so counting `..` levels breaks
/// whenever the client moves. Walk up to the directory holding `Package.swift` instead.
enum RepoRoot {
    static func file(
        _ relativePath: String, from filePath: String = #filePath
    ) -> URL {
        var directory = URL(fileURLWithPath: filePath).deletingLastPathComponent()
        while directory.path != "/" {
            let manifest = directory.appendingPathComponent("Package.swift")
            if FileManager.default.fileExists(atPath: manifest.path) {
                return directory.appendingPathComponent(relativePath)
            }
            directory = directory.deletingLastPathComponent()
        }
        fatalError("no Package.swift above \(filePath)")
    }
}
