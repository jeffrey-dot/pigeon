import Foundation

/// Liveness of one host connection. An ssh link that dies without a FIN (laptop sleep, Wi-Fi
/// switch) leaves the socket "open" forever; nothing arrives and nothing fails. The window
/// pings after `interval` of silence and calls the link dead after `timeout` of it, which
/// hands the connection to the reconnect path. Pure (time is passed in) so it is unit-tested.
struct Heartbeat: Equatable {
    static let interval: TimeInterval = 15
    static let timeout: TimeInterval = 45

    enum Action: Equatable {
        case none
        case ping
        case dead
    }

    private(set) var lastHeard: Date
    private(set) var lastPing: Date?

    init(now: Date) {
        lastHeard = now
    }

    /// Any frame from the host proves the link is up, not only `pong`.
    mutating func heard(at now: Date) {
        lastHeard = now
        lastPing = nil
    }

    /// What to do on a timer tick.
    mutating func tick(at now: Date) -> Action {
        let silent = now.timeIntervalSince(lastHeard)
        // A clock that jumped backwards (manual change) counts as fresh traffic.
        if silent < 0 {
            lastHeard = now
            return .none
        }
        if silent >= Self.timeout { return .dead }
        guard silent >= Self.interval else { return .none }
        if let lastPing, now.timeIntervalSince(lastPing) < Self.interval { return .none }
        lastPing = now
        return .ping
    }
}
