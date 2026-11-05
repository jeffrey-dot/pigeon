import Foundation

/// Opening is a separate policy from retaining terminal metadata. Only a deliberate
/// Command-click may hand an HTTP(S) URL to the system browser.
enum TerminalLinks {
    static func openingURL(_ uri: String?) -> URL? {
        guard let uri, !uri.isEmpty, uri.utf8.count <= 2048,
              !uri.contains("\\"), !uri.unicodeScalars.contains(where: forbidden),
              let decoded = uri.removingPercentEncoding, !decoded.contains("\\"),
              !decoded.unicodeScalars.contains(where: { forbidden($0) && $0 != " " }),
              let parts = URLComponents(string: uri),
              let scheme = parts.scheme?.lowercased(), ["http", "https"].contains(scheme),
              let host = parts.host, !host.isEmpty,
              parts.user == nil, parts.password == nil,
              parts.port.map({ (1...65535).contains($0) }) ?? true,
              let url = parts.url
        else { return nil }
        return url
    }

    private static func forbidden(_ scalar: Unicode.Scalar) -> Bool {
        CharacterSet.controlCharacters.contains(scalar)
            || CharacterSet.whitespacesAndNewlines.contains(scalar)
            || (0x202a...0x202e).contains(scalar.value)
            || (0x2066...0x2069).contains(scalar.value)
    }
}
