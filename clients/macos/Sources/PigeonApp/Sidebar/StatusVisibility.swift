import Foundation

/// View facts, not terminal contents: selection and a cached snapshot alone are insufficient.
struct StatusVisibility {
    var activeHost: String
    var displayedHost: String?
    var selectedSession: Int?
    var displayedSession: Int?
    var ready: Bool
    var windowFocused: Bool
    var windowVisible: Bool
    var presentedPanes: Set<UInt32>

    func panes(for host: String, session: Int?) -> Set<UInt32> {
        guard ready, windowFocused, windowVisible, let session,
              host == activeHost, host == displayedHost,
              session == selectedSession, session == displayedSession else { return [] }
        return presentedPanes
    }
}
