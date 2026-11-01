import Foundation

/// A release version as Cargo writes it, `major.minor.patch`. Missing parts count as 0, so
/// `1.2` and `1.2.0` are the same release. Anything else (`1.2.0-beta`, `v1.2`) is not a
/// version: an update is only offered between two that parse.
public struct AppVersion: Comparable, CustomStringConvertible {
    private let parts: [Int]

    public init?(_ text: String) {
        let pieces = text.split(separator: ".", omittingEmptySubsequences: false)
        guard (1...4).contains(pieces.count) else { return nil }
        var parts: [Int] = []
        for piece in pieces {
            guard !piece.isEmpty, piece.count <= 9,
                  piece.unicodeScalars.allSatisfy({ ("0"..."9").contains($0) }),
                  let number = Int(piece)
            else { return nil }
            parts.append(number)
        }
        self.parts = parts
    }

    public var description: String { parts.map(String.init).joined(separator: ".") }

    private func padded(to count: Int) -> [Int] {
        parts + Array(repeating: 0, count: max(0, count - parts.count))
    }

    public static func == (lhs: AppVersion, rhs: AppVersion) -> Bool {
        let count = max(lhs.parts.count, rhs.parts.count)
        return lhs.padded(to: count) == rhs.padded(to: count)
    }

    public static func < (lhs: AppVersion, rhs: AppVersion) -> Bool {
        let count = max(lhs.parts.count, rhs.parts.count)
        return lhs.padded(to: count).lexicographicallyPrecedes(rhs.padded(to: count))
    }
}
