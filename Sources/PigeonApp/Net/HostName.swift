import Foundation

/// What the user types into Connect to a Server, turned into an ssh destination.
///
/// People paste the command they would run (`ssh user@host`), so a leading `ssh` is dropped.
/// Anything that still has spaces or options in it is not a destination: ssh would refuse it
/// with a message about invalid characters, so it is refused here, with words a user can act on.
enum HostName {
    enum Problem: Error, CustomStringConvertible, Equatable {
        case empty
        /// Spaces, or an option like `-p 2222`: ssh takes one destination, nothing else.
        case notOneName(String)

        var description: String {
            switch self {
            case .empty:
                return "Enter the server to connect to."
            case let .notOneName(text):
                return "“\(text)” isn't a server name. Enter only the server, like user@example.com. "
                    + "To use a different port or key, add the server to your SSH config and enter its name."
            }
        }
    }

    static func parse(_ input: String) -> Result<String, Problem> {
        var text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("ssh ") || text.hasPrefix("ssh\t") {
            text = String(text.dropFirst(4)).trimmingCharacters(in: .whitespaces)
        }
        // `ssh` alone is the command with nothing after it, not a machine called ssh.
        guard !text.isEmpty, text != "ssh" else { return .failure(.empty) }
        let unusable = text.unicodeScalars.contains {
            CharacterSet.whitespacesAndNewlines.contains($0) || CharacterSet.controlCharacters.contains($0)
        }
        guard !unusable, !text.hasPrefix("-") else { return .failure(.notOneName(text)) }
        return .success(text)
    }

    /// A saved host list cleaned the same way: unusable entries are dropped, duplicates kept once.
    static func cleaned(_ hosts: [String]) -> [String] {
        var seen = Set<String>()
        return hosts.compactMap { try? parse($0).get() }.filter { seen.insert($0).inserted }
    }
}
