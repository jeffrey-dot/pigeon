import AppKit

/// Sidebar palette lifted from harp's token table (`docs/ui.md` §7.2, source
/// `app/src/workspace/view/vertical_tabs.rs` `mod sidebar::colors`).
///
/// Every token is a dynamic color with a light and a dark branch, so flipping the
/// system appearance repaints without a restart. Only the tokens pigeon draws with
/// are listed; harp's menu/lifted/card tokens are not part of this UI.
enum SidebarTheme {
    /// Row titles and the right-pane session name.
    static let ink = dynamic(light: 0x1d1d1f, dark: 0xf5f5f7)
    /// Row age, right-pane path/state, header button glyphs.
    static let gray = dynamic(light: 0x6e6e73, dark: 0x8e8e93)
    static let sectionName = dynamic(light: 0x7c7c82, dark: 0x8e8e93)
    static let sectionCount = dynamic(light: 0xa1a1a6, dark: 0x636366)
    /// `N needs you` — orange in both appearances.
    static let needsText = dynamic(light: 0x9a5700, dark: 0xf0b35c)
    /// The row's "waiting" icon ring and dot (`VT:9222-9223`). Note these differ from
    /// `NEEDS_RING`/`NEEDS_DOT`, which harp defines but does not use for rows.
    static let iconNeedsRing = dynamic(light: 0xc98314, dark: 0xe8a33d)
    /// harp's fixed `needs_you_color()` (`VT:4447`): the waiting row's age, same in both
    /// appearances because a themed yellow reads as "done" in several themes.
    static let needsAccent = dynamic(light: 0xe8a33d, dark: 0xe8a33d)
    /// The working arc and the filled unread-done disc.
    static let iconStrong = dynamic(light: 0x3a3a3c, dark: 0xe5e5ea)
    /// Read-done tick, idle moon, empty agent ring.
    static let ring = dynamic(light: 0xa1a1a6, dark: 0x636366)
    /// Selected row surface.
    static let selected = dynamic(light: 0xffffff, dark: 0x3a3a3d)
    /// Selected while the window is in the background.
    static let quietSelected = dynamic(light: 0xf4f4f5, dark: 0x2f2f32)
    static let selectedEdge = overlay(light: 0x000000, alpha: 20, dark: 0xffffff, alpha: 26)
    /// The selected row's drop shadow (`VT:9211` `SELECTED_SHADOW`).
    static let selectedShadow = overlay(light: 0x000000, alpha: 15, dark: 0x000000, alpha: 64)
    /// The sidebar's own surface, under the window's material tint.
    static let sidebar = dynamic(light: 0xe8e8eb, dark: 0x232326)
    static let hover = overlay(light: 0x000000, alpha: 13, dark: 0xffffff, alpha: 15)
    /// The card's 0.5pt edge and its shadow (`VT:9226-9227`).
    static let cardEdge = overlay(light: 0x000000, alpha: 26, dark: 0xffffff, alpha: 26)
    static let cardShadow = overlay(light: 0x000000, alpha: 13, dark: 0x000000, alpha: 64)
    /// harp's card is painted with the theme background (`view.rs:22241`), so the card and
    /// the terminal are one surface in both appearances: white under a light theme, near
    /// black under a dark one (`default_themes.rs:262/275`).
    static let terminalBackground = dynamic(light: 0xffffff, dark: 0x050505)
    /// The default text colour for cells that carry none (`theme.foreground()`).
    static let terminalForeground = dynamic(light: 0x111111, dark: 0xffffff)
    /// The card header's hairline (`VT:9212`), same token as harp's `DIVIDER`.
    static let divider = dynamic(light: 0xececef, dark: 0x3a3a3c)

    /// harp folds a failed agent into `Done`; pigeon keeps `error` distinct, so this
    /// one token has no harp counterpart and stays a system color.
    static let error = NSColor.systemRed

    /// `needs you` count text: same wording and color as the group header in harp.
    static func needsLabel(_ count: Int) -> String {
        count == 1 ? "1 needs you" : "\(count) need you"
    }

    private static func dynamic(light: UInt32, dark: UInt32) -> NSColor {
        NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return color(isDark ? dark : light, alpha: 255)
        }
    }

    private static func overlay(light: UInt32, alpha: UInt8, dark: UInt32, alpha darkAlpha: UInt8) -> NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
                ? color(dark, alpha: darkAlpha)
                : color(light, alpha: alpha)
        }
    }

    private static func color(_ hex: UInt32, alpha: UInt8) -> NSColor {
        NSColor(
            srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
            green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255,
            alpha: CGFloat(alpha) / 255
        )
    }
}
