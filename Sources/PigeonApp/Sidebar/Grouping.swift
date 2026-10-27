import Foundation

/// Project grouping rules from `docs/ui.md`: a session belongs to its git root
/// when it has one, otherwise to the last component of its cwd.
///
/// Pure: the caller supplies the git root (the app looks it up), which keeps this
/// testable without touching the file system.
public enum Grouping {
    public static func groupName(forCwd cwd: String, gitRoot: String? = nil) -> String {
        if let root = gitRoot, !root.isEmpty {
            return lastComponent(root)
        }
        return lastComponent(cwd)
    }

    /// "…/pigeon/core" → "core"; "/" → "/".
    public static func lastComponent(_ path: String) -> String {
        let trimmed = path.hasSuffix("/") && path.count > 1
            ? String(path.dropLast())
            : path
        let name = (trimmed as NSString).lastPathComponent
        return name.isEmpty ? "/" : name
    }
}
