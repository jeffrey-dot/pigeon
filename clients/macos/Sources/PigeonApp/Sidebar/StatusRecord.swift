import Foundation

/// Status seq belongs to a pane within one host instance, independently of screen seq.
/// The record id distinguishes reports even when an old host omits it (empty string).
struct StatusRecord: Codable, Equatable, Hashable {
    var seq: UInt32
    var record: String
}
