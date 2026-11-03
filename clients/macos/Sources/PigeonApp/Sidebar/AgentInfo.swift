import Foundation

/// What an agent last said about itself through OSC 7501, as the host forwards it in
/// `status` (protocol §3): who it is, what it is doing or waiting for, and since when.
/// Every string is program-controlled text and is only ever shown as plain text.
public struct AgentInfo: Equatable {
    /// `app=`, e.g. `claude`; empty when the agent did not name itself.
    public var app: String
    /// The task or the question (`msg=`), already decoded and free of control characters.
    public var message: String
    /// `permission` / `question` / `auth` while blocked; empty otherwise.
    public var kind: String
    /// When the agent entered its current state, on this machine's clock.
    public var since: Date

    public init(app: String = "", message: String = "", kind: String = "", since: Date = Date()) {
        self.app = app
        self.message = message
        self.kind = kind
        self.since = since
    }

    /// The host reports a duration (`age_ms`) so the two clocks never have to agree; turn it
    /// into a start time here. Without one (an older host), the arrival time stands in.
    public init(app: String, message: String, kind: String, ageMs: UInt64?, now: Date = Date()) {
        let age = ageMs.map { TimeInterval($0) / 1000 } ?? 0
        self.init(app: app, message: message, kind: kind, since: now.addingTimeInterval(-age))
    }

    public static let none = AgentInfo(since: .distantPast)
}
