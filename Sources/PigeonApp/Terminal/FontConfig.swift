import Foundation

/// The terminal font, read from `~/.config/pigeon/config.toml`:
///
///     font-family = "JetBrains Mono"
///     font-size = 14
///     line-height = 1.3
///
/// Only flat `key = value` lines are understood (no tables or arrays), which is all this file
/// holds; anything else, an unknown key or a value out of range, is ignored and the default
/// kept, so a typo never leaves the terminal unreadable. Pure: parsing is unit-tested.
struct FontConfig: Equatable {
    /// Empty means the system monospace (SF Mono).
    var family = ""
    var size: Double = 13
    var lineHeight: Double = 1.25

    static let sizeRange = 8.0...36.0
    static let lineHeightRange = 1.0...2.0

    init(family: String = "", size: Double = 13, lineHeight: Double = 1.25) {
        self.family = family
        self.size = size
        self.lineHeight = lineHeight
    }

    init(toml text: String) {
        self.init()
        for raw in text.split(whereSeparator: \.isNewline) {
            let line = Self.stripComment(String(raw)).trimmingCharacters(in: .whitespaces)
            guard let equals = line.firstIndex(of: "=") else { continue }
            let key = line[..<equals].trimmingCharacters(in: .whitespaces)
            let value = line[line.index(after: equals)...].trimmingCharacters(in: .whitespaces)
            switch key {
            case "font-family":
                if let name = Self.string(value) { family = name }
            case "font-size":
                if let number = Double(value), Self.sizeRange.contains(number) { size = number }
            case "line-height":
                if let number = Double(value), Self.lineHeightRange.contains(number) { lineHeight = number }
            default:
                continue
            }
        }
    }

    static var defaultURL: URL {
        if let override = ProcessInfo.processInfo.environment["PIGEON_CONFIG_DIR"] {
            return URL(fileURLWithPath: override).appendingPathComponent("config.toml")
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config/pigeon/config.toml")
    }

    static func load(from url: URL = defaultURL) -> FontConfig {
        guard let data = try? Data(contentsOf: url), let text = String(data: data, encoding: .utf8)
        else { return FontConfig() }
        return FontConfig(toml: text)
    }

    /// A basic TOML string: `"…"` without escapes beyond `\"`. Control characters refused.
    private static func string(_ value: String) -> String? {
        guard value.count >= 2, value.first == "\"", value.last == "\"" else { return nil }
        let inner = String(value.dropFirst().dropLast()).replacingOccurrences(of: "\\\"", with: "\"")
        guard !inner.unicodeScalars.contains(where: { $0.properties.generalCategory == .control })
        else { return nil }
        return inner
    }

    /// Drops a `#` comment that is not inside a string.
    private static func stripComment(_ line: String) -> String {
        var quoted = false
        var out = ""
        for character in line {
            if character == "\"" { quoted.toggle() }
            if character == "#", !quoted { break }
            out.append(character)
        }
        return out
    }
}
