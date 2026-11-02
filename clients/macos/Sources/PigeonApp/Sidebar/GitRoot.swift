import Foundation

/// Finds the project root by walking up to the nearest `.git` entry. Pure enough
/// to test with a temp directory; the sidebar only calls it when grouping.
enum GitRoot {
    /// `onThisMac`: the path is on this machine's disk. A remote session's cwd names a
    /// directory on another host; probing the same path here would group it by whatever
    /// happens to exist locally (and touch the disk for nothing), so it falls back to the
    /// cwd's last component.
    static func find(cwd: String, onThisMac: Bool = true) -> String? {
        guard onThisMac, !cwd.isEmpty else { return nil }
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
