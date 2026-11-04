import Foundation

/// The wire ack omits host/instance/id; retain them until choosing the live connection.
struct StatusAcknowledgement: Equatable {
    var host: String
    var instance: String
    var pane: UInt32
    var record: StatusRecord

    func matches(host: String, instance: String?) -> Bool {
        self.host == host && self.instance == instance
    }
}
