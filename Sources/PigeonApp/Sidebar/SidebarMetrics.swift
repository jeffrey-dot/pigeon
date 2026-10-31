import CoreGraphics

/// Sidebar and window-chrome numbers from harp (`docs/ui.md` §7.1 / §7.3, source
/// `app/src/workspace/view/vertical_tabs.rs` `mod sidebar` and
/// `app/src/util/traffic_lights.rs`).
///
/// One place for every size the sidebar draws with, so the views carry no bare numbers.
enum SidebarMetrics {
    static let width: CGFloat = 272
    static let widthRange: ClosedRange<CGFloat> = 200...360

    /// The row that hosts the traffic lights and the collapse/new buttons.
    static let headerHeight: CGFloat = 56
    static let listSidePadding: CGFloat = 10
    static let listTopPadding: CGFloat = 4
    static let sectionGap: CGFloat = 14
    static let rowGap: CGFloat = 1

    static let rowHeight: CGFloat = 30
    /// A row whose agent says what it is doing gets a second, smaller line.
    static let rowHeightWithDetail: CGFloat = 44
    static let rowDetailSize: CGFloat = 11
    static let rowRadius: CGFloat = 8
    static let rowInsetLeft: CGFloat = 12
    static let rowInsetRight: CGFloat = 8
    static let rowSpacing: CGFloat = 8
    /// Half-pixel inset so the 0.5pt selected edge is not clipped by the row bounds.
    static let rowPillInset: CGFloat = 0.25
    static let selectedShadowBlur: CGFloat = 2
    static let selectedShadowOffsetY: CGFloat = -1

    static let sectionHeaderHeight: CGFloat = 28
    /// The machine heading above a host's project groups, and the extra gap before the next
    /// machine so two hosts read as two blocks.
    static let machineHeaderHeight: CGFloat = 22
    static let machineGap: CGFloat = 18
    static let sectionInsetLeft: CGFloat = 12
    static let sectionInsetRight: CGFloat = 9

    /// harp gives every row a 24pt circular icon with the status on its corner
    /// (`VERTICAL_TABS_ICON_SIZE`, `ICON_WITH_STATUS_GAP`).
    static let rowIconSize: CGFloat = 24
    static let rowIconGap: CGFloat = 8
    static let statusBadgeSize: CGFloat = 13
    static let statusBadgeRing: CGFloat = 2
    /// The hover `×` chip harp shows in place of the time (`VT` `row_close`, 18pt / radius 5).
    static let rowCloseSize: CGFloat = 18
    static let rowCloseRadius: CGFloat = 5
    static let groupActionSize: CGFloat = 18
    static let groupActionRadius: CGFloat = 5

    /// The 16pt slot a 15pt status glyph sits in.
    static let statusGlyphSlot: CGFloat = 16
    static let statusGlyphSize: CGFloat = 15
    static let unreadDotSize: CGFloat = 5

    static let headerButtonSize: CGFloat = 28
    static let headerButtonRadius: CGFloat = 7
    static let headerButtonIconSize: CGFloat = 17
    static let headerButtonSpacing: CGFloat = 2
    /// harp pads the header buttons 14pt from the sidebar edge.
    static let headerPadRight: CGFloat = 14
    /// Gap between a section's name and its count, and between other inline labels.
    static let sectionSpacing: CGFloat = 6
    /// Gap between the unread dot and the title.
    static let unreadDotGap: CGFloat = 5

    static let cardHeaderHeight: CGFloat = 40
    static let cardHeaderPadOpen: CGFloat = 32
    static let cardHeaderPadClosed: CGFloat = 84
    static let cardHeaderPadRight: CGFloat = 20
    static let cardHeaderSpacing: CGFloat = 10
    static let dividerWidth: CGFloat = 0.5

    /// The right pane is a card, not a flush pane: 10pt radius, 8pt inset on the three
    /// window edges, a 0.5pt edge and a soft shadow (`docs/ui.md` §7.1).
    static let cardRadius: CGFloat = 10
    static let cardInset: CGFloat = 8
    static let cardEdgeWidth: CGFloat = 0.5
    static let cardShadowOffsetY: CGFloat = 4
    static let cardShadowBlur: CGFloat = 16
    static let cardContentLeftPadding: CGFloat = 32

    static let rowTitleSize: CGFloat = 13
    static let rowTimeSize: CGFloat = 10
    static let sectionTitleSize: CGFloat = 11.5
    static let sectionCountSize: CGFloat = 11.5
    static let machineTitleSize: CGFloat = 11
    static let needsLabelSize: CGFloat = 12
    static let cardTitleSize: CGFloat = 12.5
    static let cardPathSize: CGFloat = 12.5
    static let cardStateSize: CGFloat = 12
}
