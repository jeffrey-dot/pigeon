import AppKit

/// The app's appearance setting: follow the system, or pin light/dark.
///
/// Pinning sets `NSApp.appearance`, and that **overrides the system until it is cleared** —
/// which is why "follow the system" has to be one of the states rather than "not set yet".
enum AppearanceMode: String, CaseIterable {
    case system
    case dark
    case light

    /// light → dark → system: the toggle always has a way back to following the system.
    var next: AppearanceMode {
        switch self {
        case .system: return .dark
        case .dark: return .light
        case .light: return .system
        }
    }

    var appearance: NSAppearance? {
        switch self {
        case .system: return nil
        case .dark: return NSAppearance(named: .darkAqua)
        case .light: return NSAppearance(named: .aqua)
        }
    }

    /// Reads `PIGEON_APPEARANCE=light|dark|system` (screenshot runs); default follows the system.
    static func fromEnvironment(_ value: String?) -> AppearanceMode {
        switch value?.lowercased() {
        case "light": return .light
        case "dark": return .dark
        default: return .system
        }
    }

    /// The mode currently in effect, inferring from `NSApp.appearance` when pinned.
    static func current(_ app: NSApplication = NSApp) -> AppearanceMode {
        guard let appearance = app.appearance else { return .system }
        return appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? .dark : .light
    }
}
