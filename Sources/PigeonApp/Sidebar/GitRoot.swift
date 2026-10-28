import Foundation

/// Finds the project root by walking up to the nearest `.git` entry. Pure enough
/// to test with a temp directory; the sidebar only calls it when grouping.
enum GitRoot {
    static func find(cwd: String) -> String? {
        guard !cwd.isEmpty else { return nil }
        var url = URL(fileURLWithPath: cwd)
        while url.path != "/" && !url.path.isEmpty {
            if FileManager.default.fileExists(atPath: url.appendingPathComponent(".git").path) {
                return url.path
            }
            url.deleteLastPathComponent()
        }
        return nil
    }
}
