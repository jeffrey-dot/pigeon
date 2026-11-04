import Foundation

/// The durable part of one host's sidebar, scoped to exactly one process lifetime.
struct HostStatusState: Codable, Equatable {
    var instance: String
    var sessions: [String: [UInt32: PaneStatus.Receipt]] = [:]
}
