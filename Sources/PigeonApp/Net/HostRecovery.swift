import Foundation

/// What the window does when the host socket goes away. Pure data so the rules can be tested
/// without AppKit or a real host.
public enum HostRecovery {
    /// Reconnect attempts before the window hands the job to the user.
    public static let attempts = 3
    /// Gap between attempts: long enough for a restarting host, short enough to feel automatic.
    public static let retryDelay: TimeInterval = 1.0
    /// A network drop is not a crashed local socket: keep re-probing ssh for about a minute
    /// before handing the problem to the user (the M4 acceptance is a 30 s outage).
    public static let remoteAttempts = 20
    public static let remoteRetryDelay: TimeInterval = 1.5

    public enum State: Equatable {
        case connected
        /// The socket is gone; a reconnect is scheduled.
        case reconnecting(attempt: Int)
        /// Out of attempts, or the host said no: only the user can fix this.
        case stopped(detail: String)
    }

    /// The banner text, or `nil` while connected (nothing to say).
    public static func banner(for state: State) -> String? {
        switch state {
        case .connected:
            return nil
        case let .reconnecting(attempt):
            return "Session host is not reachable — reconnecting (\(attempt)/\(attempts))…"
        case let .stopped(detail):
            return detail
        }
    }

    /// Whether another attempt is worth making after `attempt` failures.
    public static func canRetry(after attempt: Int) -> Bool {
        attempt < attempts
    }

    /// Same question for a remote probe, which needs a much longer budget.
    public static func canRetryRemote(after attempt: Int) -> Bool {
        attempt < remoteAttempts
    }

    /// What the user can do about it; `nil` while a retry is pending.
    public static func action(for state: State) -> String? {
        switch state {
        case .connected, .reconnecting:
            return nil
        case .stopped:
            return "Retry"
        }
    }

    /// Which way a lost connection comes back: a remote session re-probes its ssh
    /// transports, a local one restarts the socket host next to us.
    public enum Plan: Equatable {
        case remote
        case local
    }

    public static func plan(hasRemote: Bool) -> Plan {
        hasRemote ? .remote : .local
    }

    /// The `attach.since` for a pane after a reconnect: the last frame it drew, if that
    /// screen is still on display. A fresh pane, or one whose host restarted (the caller
    /// then forgets `drawn`), gets a plain snapshot.
    public static func resumeSeq(drawn: UInt32?, stale: Bool, onScreen: Bool) -> UInt32? {
        guard stale, onScreen else { return nil }
        return drawn
    }

    /// The command line shown when no host binary can be found to start automatically.
    public static let startCommand = "cargo run --manifest-path core/Cargo.toml --bin pigeon-host"
}
