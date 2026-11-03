import Foundation

/// Carries a `+` dropdown action through `NSMenuItem.representedObject`, which takes objects.
final class NewSessionChoice: NSObject {
    let action: SidebarModel.NewSessionItem.Action

    init(_ action: SidebarModel.NewSessionItem.Action) {
        self.action = action
    }
}
