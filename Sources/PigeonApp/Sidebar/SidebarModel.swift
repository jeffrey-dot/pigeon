import Foundation

/// One row of the sidebar. Everything here is derived data — no AppKit, no IO.
public struct SidebarSession: Equatable {
    /// Urgency order: `blocked` first, matching `docs/ui.md`.
    public enum Status: Int, Comparable, CaseIterable {
        case blocked = 0
        case error = 1
        case done = 2
        case working = 3
        case idle = 4

        public static func < (lhs: Status, rhs: Status) -> Bool {
            lhs.rawValue < rhs.rawValue
        }
    }

    public var id: Int
    public var title: String
    public var group: String
    public var status: Status
    public var unread: Int

    public init(id: Int, title: String, group: String, status: Status, unread: Int = 0) {
        self.id = id
        self.title = title
        self.group = group
        self.status = status
        self.unread = unread
    }
}

/// Grouping, ordering and the `needs you` count. Kept free of AppKit so it can be
/// unit tested and reused by a future Linux client.
public enum SidebarModel {
    /// Groups in first-seen order; rows inside a group by urgency, then title.
    public static func sections(_ sessions: [SidebarSession]) -> [(group: String, sessions: [SidebarSession])] {
        var order: [String] = []
        var grouped: [String: [SidebarSession]] = [:]
        for session in sessions {
            if grouped[session.group] == nil {
                order.append(session.group)
            }
            grouped[session.group, default: []].append(session)
        }
        return order.map { group in
            let rows = (grouped[group] ?? []).sorted { left, right in
                if left.status != right.status { return left.status < right.status }
                return left.title.localizedStandardCompare(right.title) == .orderedAscending
            }
            return (group: group, sessions: rows)
        }
    }

    /// Sessions that want the user: blocked, errored, or finished-and-unread.
    public static func needsYou(_ sessions: [SidebarSession]) -> Int {
        sessions.filter { session in
            switch session.status {
            case .blocked, .error:
                return true
            case .done:
                return session.unread > 0
            case .working, .idle:
                return false
            }
        }.count
    }

    /// `"Working · 1m"` — the relative-time line from the reference UI.
    public static func subtitle(for session: SidebarSession, now: Date = Date(), since: Date) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(since)))
        let relative: String
        switch seconds {
        case ..<60: relative = "\(seconds)s"
        case ..<3600: relative = "\(seconds / 60)m"
        default: relative = "\(seconds / 3600)h"
        }
        let state: String
        switch session.status {
        case .blocked: state = "Needs you"
        case .error: state = "Error"
        case .done: state = "Done"
        case .working: state = "Working"
        case .idle: state = "Idle"
        }
        return "\(state) · \(relative)"
    }
}
