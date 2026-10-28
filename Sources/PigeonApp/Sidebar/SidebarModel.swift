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
    /// Application running in the session's active pane (OSC 0/2 title, or the shell).
    public var app: String
    /// Working directory reported by the pane (OSC 7) or the spawn cwd.
    public var cwd: String
    public var lastSeen: Date

    public init(
        id: Int,
        title: String,
        group: String,
        status: Status,
        unread: Int = 0,
        app: String = "shell",
        cwd: String = "",
        lastSeen: Date = Date()
    ) {
        self.id = id
        self.title = title
        self.group = group
        self.status = status
        self.unread = unread
        self.app = app
        self.cwd = cwd
        self.lastSeen = lastSeen
    }
}

/// Grouping, ordering and the `needs you` count. Kept free of AppKit so it can be
/// unit tested and reused by a future Linux client.
public enum SidebarModel {
    /// Groups in first-seen order; rows inside a group by urgency, then title.
    public static func sections(
        _ sessions: [SidebarSession],
        order: [Int] = []
    ) -> [(group: String, sessions: [SidebarSession])] {
        var groups: [String] = []
        var grouped: [String: [SidebarSession]] = [:]
        for session in sessions {
            if grouped[session.group] == nil {
                groups.append(session.group)
            }
            grouped[session.group, default: []].append(session)
        }
        func rank(_ id: Int) -> Int { order.firstIndex(of: id) ?? Int.max }
        return groups.map { group in
            let rows = (grouped[group] ?? []).sorted { left, right in
                if left.status != right.status { return left.status < right.status }
                let (leftRank, rightRank) = (rank(left.id), rank(right.id))
                if leftRank != rightRank { return leftRank < rightRank }
                return left.title.localizedStandardCompare(right.title) == .orderedAscending
            }
            return (group: group, sessions: rows)
        }
    }

    /// Maps a protocol `status.state` string onto the sidebar enum.
    public static func status(from text: String) -> SidebarSession.Status? {
        switch text {
        case "idle": return .idle
        case "working": return .working
        case "blocked": return .blocked
        case "done": return .done
        case "error": return .error
        default: return nil
        }
    }

    /// Session the user should jump to next (blocked → error → unread done).
    public static func nextNeedsYou(_ sessions: [SidebarSession], after id: Int?) -> SidebarSession? {
        let ordered = sessions.sorted { $0.id < $1.id }
        guard !ordered.isEmpty else { return nil }
        let start = ordered.firstIndex { $0.id == id }.map { ($0 + 1) % ordered.count } ?? 0
        for offset in 0..<ordered.count {
            let candidate = ordered[(start + offset) % ordered.count]
            if candidate.status == .blocked || candidate.status == .error
                || (candidate.status == .done && candidate.unread > 0)
            {
                return candidate
            }
        }
        return nil
    }

    /// The line under a row's title: app, state and age.
    public static func rowSubtitle(for session: SidebarSession, now: Date = Date()) -> String {
        "\(session.app) · \(subtitle(for: session, now: now, since: session.lastSeen))"
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
