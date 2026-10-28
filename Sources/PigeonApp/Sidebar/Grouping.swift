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
        // A remote session spends its time in `$HOME`, and we only know *this* machine's
        // home path, so both layouts are recognised: `/Users/<user>` and `/home/<user>`.
        if isHome(cwd) {
            return "~"
        }
        return lastComponent(cwd)
    }

    /// The group a session belongs to: decided **once**, when the session first appears, and
    /// left alone afterwards. `cd` must never move a session between the user's groups.
    public static func pinnedGroup(
        existing: String?, cwd: String, gitRoot: String? = nil
    ) -> String {
        if let existing, !existing.isEmpty {
            return existing
        }
        guard !cwd.isEmpty else { return "~" }
        return groupName(forCwd: cwd, gitRoot: gitRoot)
    }

    /// `/Users/jeffrey`, `/home/jeffrey`, or a bare `~`.
    public static func isHome(_ path: String) -> Bool {
        let trimmed = path.hasSuffix("/") && path.count > 1
            ? String(path.dropLast())
            : path
        if trimmed == "~" { return true }
        let parts = trimmed.split(separator: "/").map(String.init)
        return parts.count == 2 && (parts[0] == "Users" || parts[0] == "home")
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
