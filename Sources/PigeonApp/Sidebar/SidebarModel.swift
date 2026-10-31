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
    /// Whether an OSC 7501 record ever arrived; without one there is no state to show.
    public var reported: Bool
    /// How the host picked this pane's size (`latest`/`smallest`/`locked`), and how many
    /// windows are watching it (protocol §6). One client means there is nothing to explain.
    public var sizeSource: String
    public var clients: Int
    /// The machine this session lives on (`local` for the socket next to us).
    public var host: String
    /// The agent's own report: name, task or question, and when it entered its state.
    public var agent: AgentInfo
    /// Every pane was restored from disk after the host restarted: the screens are what was
    /// there, the processes are gone.
    public var restored: Bool


    public init(
        id: Int,
        title: String,
        group: String,
        status: Status,
        unread: Int = 0,
        app: String = "shell",
        cwd: String = "",
        lastSeen: Date = Date(),
        reported: Bool = false,
        sizeSource: String = "",
        clients: Int = 1,
        host: String = "local",
        agent: AgentInfo = .none,
        restored: Bool = false
    ) {
        self.id = id
        self.title = title
        self.group = group
        self.status = status
        self.unread = unread
        self.app = app
        self.cwd = cwd
        self.lastSeen = lastSeen
        self.reported = reported
        self.sizeSource = sizeSource
        self.clients = clients
        self.host = host
        self.agent = agent
        self.restored = restored
    }
}

/// One project heading in the sidebar, under the machine it lives on.
public struct SidebarSection: Equatable {
    /// The machine (`local` for this one); consecutive sections share a machine heading.
    public var host: String
    /// Identity for renames: `host / project`, so the same project on two machines stays two
    /// groups that rename independently.
    public var key: String
    /// The project name the heading shows.
    public var group: String
    public var sessions: [SidebarSession]

    public init(host: String, key: String, group: String, sessions: [SidebarSession]) {
        self.host = host
        self.key = key
        self.group = group
        self.sessions = sessions
    }
}

/// Grouping, ordering and the `needs you` count. Kept free of AppKit so it can be
/// unit tested and reused by a future Linux client.
public enum SidebarModel {
    /// Machines in first-seen order, their groups in first-seen order; rows inside a group
    /// by urgency, then title.
    public static func sections(
        _ sessions: [SidebarSession],
        order: [Int] = []
    ) -> [SidebarSection] {
        var hosts: [String] = []
        var keys: [String: [String]] = [:]
        var grouped: [String: [SidebarSession]] = [:]
        for session in sessions {
            let host = session.host.isEmpty ? "local" : session.host
            let key = groupLabel(host: host, project: session.group)
            if keys[host] == nil {
                hosts.append(host)
            }
            if grouped[key] == nil {
                keys[host, default: []].append(key)
            }
            grouped[key, default: []].append(session)
        }
        func rank(_ id: Int) -> Int { order.firstIndex(of: id) ?? Int.max }
        return hosts.flatMap { host in
            (keys[host] ?? []).map { key in
                let members = grouped[key] ?? []
                let rows = members.sorted { left, right in
                    if left.status != right.status { return left.status < right.status }
                    let (leftRank, rightRank) = (rank(left.id), rank(right.id))
                    if leftRank != rightRank { return leftRank < rightRank }
                    return left.title.localizedStandardCompare(right.title) == .orderedAscending
                }
                return SidebarSection(
                    host: host, key: key, group: members.first?.group ?? key, sessions: rows)
            }
        }
    }

    /// One item of the sidebar's `+` dropdown (`docs/ui.md` §2.3).
    public struct NewSessionItem: Equatable {
        public enum Action: Equatable {
            /// `new_session` on this host (`"local"` is this Mac).
            case newSession(host: String)
            /// The host is known but not connected: connect it (its sessions come with it).
            case connect(host: String)
            /// Ask for an ssh destination (`Connect to Host…`).
            case connectNew
        }

        public var title: String
        public var action: Action
        public var enabled: Bool
    }

    /// This Mac first, then every saved ssh host in the order it was added, then a way to add
    /// one. A host still connecting is listed but disabled, so a click cannot race the probe.
    public static func newSessionItems(
        hosts: [String], connected: Set<String>, connecting: String?, localName: String
    ) -> [NewSessionItem] {
        var items = [NewSessionItem(
            title: "New Session on \(machineLabel(host: "local", localName: localName))",
            action: .newSession(host: "local"), enabled: true)]
        for host in hosts where host != "local" {
            if connected.contains(host) {
                items.append(NewSessionItem(
                    title: "New Session on \(host)", action: .newSession(host: host), enabled: true))
            } else if host == connecting {
                items.append(NewSessionItem(
                    title: "\(host) · connecting", action: .connect(host: host), enabled: false))
            } else {
                items.append(NewSessionItem(
                    title: "Connect to \(host)", action: .connect(host: host), enabled: true))
            }
        }
        items.append(NewSessionItem(title: "Connect to Host…", action: .connectNew, enabled: true))
        return items
    }

    /// The machine heading above a host's groups: this computer's name for `local`, otherwise
    /// what the user typed to reach it (ssh alias or address).
    public static func machineLabel(host: String, localName: String) -> String {
        if host.isEmpty || host == "local" {
            return localName.isEmpty ? "This Mac" : localName
        }
        return host
    }

    /// A group's rename key: the project alone on this machine, `host / project` once the row
    /// comes from somewhere else (`docs/remote.md` §4).
    public static func groupLabel(host: String, project: String) -> String {
        if host.isEmpty || host == "local" {
            return project
        }
        return "\(host) / \(project)"
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

    /// The row's second line: what the agent is doing or what it is waiting for. Only live
    /// states get one, so a quiet sidebar stays one line per session; `nil` = single line.
    public static func detailLine(for session: SidebarSession) -> String? {
        // Said plainly, so a row from before a reboot is not mistaken for a live one.
        if session.restored { return "Ended · restored after host restart" }
        let text = session.agent.message.trimmingCharacters(in: .whitespacesAndNewlines)
        switch session.status {
        case .blocked:
            if !text.isEmpty { return text }
            return session.agent.kind.isEmpty ? "Waiting for you" : "Waiting · \(session.agent.kind)"
        case .working:
            return text.isEmpty ? nil : text
        case .error:
            return text.isEmpty ? nil : text
        case .done, .idle:
            return nil
        }
    }

    /// The card header's state: `Needs you · 2m · Run tests?`; blank when the pane never
    /// reported, because there is no state to describe.
    public static func headerBadge(for session: SidebarSession, now: Date = Date()) -> String {
        if session.restored { return "Ended · restored after host restart · ⌘⇧W to close" }
        guard session.reported, session.status != .idle else { return "" }
        let base = subtitle(status: session.status, since: session.lastSeen, now: now)
        guard let line = detailLine(for: session) else { return base }
        return "\(base) · \(line)"
    }

    /// The row's name: what the user called it, else the agent that reported, else the program.
    public static func rowTitle(for session: SidebarSession) -> String {
        if session.title != session.app, !session.title.isEmpty { return session.title }
        if !session.agent.app.isEmpty { return session.agent.app }
        return session.app
    }

    /// Whether closing a session left nothing to show: no session on any connected machine,
    /// and no `new_session` still waiting for its answer.
    public static func shouldQuitAfterClose(remainingSessions: Int, creatingSession: Bool) -> Bool {
        remainingSessions == 0 && !creatingSession
    }

    /// The line under a row's title: app, state and age.
    public static func rowSubtitle(for session: SidebarSession, now: Date = Date()) -> String {
        subtitle(for: session, now: now, since: session.lastSeen)
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
        subtitle(status: session.status, since: since, now: now)
    }

    /// Same wording for the right-pane badge, which only has the raw status.
    public static func subtitle(status: SidebarSession.Status, since: Date, now: Date = Date()) -> String {
        let state: String
        switch status {
        case .blocked: state = "Needs you"
        case .error: state = "Error"
        case .done: state = "Done"
        case .working: state = "Working"
        case .idle: state = "Idle"
        }
        return "\(state) · \(relativeTime(since, now: now))"
    }

    /// harp capitalizes only the first character of a group name (`VT:10063`), not every word.
    public static func displayGroupName(_ name: String) -> String {
        guard let first = name.first else { return name }
        return first.uppercased() + name.dropFirst()
    }

    /// `now` / `1m` / `3h` / `2d` — the row age from `docs/ui.md` §2.2.
    public static func relativeTime(_ date: Date, now: Date = Date()) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(date)))
        switch seconds {
        case ..<60: return "now"
        case ..<3600: return "\(seconds / 60)m"
        case ..<86_400: return "\(seconds / 3600)h"
        default: return "\(seconds / 86_400)d"
        }
    }

    /// The row's trailing age; empty when the pane never reported, because the only
    /// date we have is when the row appeared — not when the program last spoke.
    /// With more than one window attached it also names who owns the size (protocol §6).
    public static func ageText(for session: SidebarSession, now: Date = Date()) -> String {
        let age = session.reported ? relativeTime(session.lastSeen, now: now) : ""
        guard session.clients > 1 else { return age }
        let owner = "\(session.clients)× \(session.sizeSource)"
        return age.isEmpty ? owner : "\(age) · \(owner)"
    }

    /// What a pane is running, for the row's leading circle icon (harp draws a per-kind icon;
    /// pigeon only knows the app name, so the mapping is by name).
    public enum AppKind: String, CaseIterable {
        case agent
        case editor
        case shell
        case other

        public var symbol: String {
            switch self {
            case .agent: return "sparkles"
            case .editor: return "chevron.left.forwardslash.chevron.right"
            case .shell: return "terminal"
            case .other: return "square.dashed"
            }
        }
    }

    public static func appKind(_ app: String) -> AppKind {
        let name = (app as NSString).lastPathComponent.lowercased()
        if ["pi", "claude", "codex", "aider", "cursor-agent"].contains(name) { return .agent }
        if ["vim", "nvim", "vi", "emacs", "nano", "hx", "helix"].contains(name) { return .editor }
        if ["zsh", "bash", "sh", "fish", "dash", "ksh", "nu"].contains(name) { return .shell }
        return .other
    }

    /// A stable tint per project, so two shells in one project still differ from another
    /// project's. Same `cwd` → same tint on every launch (`SidebarTheme.appTints.count` buckets).
    public static func tintIndex(cwd: String, buckets: Int = 6) -> Int {
        var hash: UInt32 = 5381
        for byte in cwd.utf8 { hash = hash &* 33 &+ UInt32(byte) }
        return Int(hash % UInt32(max(buckets, 1)))
    }

    /// The row's trailing icon as an SF Symbol (`docs/ui.md` §2.2/§7.4); `idle` has none.
    public static func statusSymbol(for session: SidebarSession) -> String? {
        switch session.status {
        case .working: return "circle.dashed"
        case .blocked: return "exclamationmark.circle"
        case .done: return session.unread > 0 ? "checkmark.circle.fill" : "checkmark"
        case .error: return "exclamationmark.triangle.fill"
        case .idle: return nil
        }
    }

    /// A row pulses once when its session turns `blocked`, not on every redraw while it
    /// waits, and not when a cell first shows a session that was already blocked (launch,
    /// scrolling) — that would make a whole list of waiting agents flash at once.
    public static func shouldPulse(from previous: SidebarSession.Status?, to next: SidebarSession.Status) -> Bool {
        guard let previous else { return false }
        return next == .blocked && previous != .blocked
    }
}
