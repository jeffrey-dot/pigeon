import Foundation

/// The outcome of one `hello` probe, shared between the waiting thread and the client's
/// read queue. A class with a lock, so a late `hello_ack` after a timeout is harmless.
final class HandshakeProbe: @unchecked Sendable {
    private let lock = NSLock()
    private let done = DispatchSemaphore(value: 0)
    private var answered = false
    private var finished = false

    /// The first call decides; later ones (a close after the ack) change nothing.
    func finish(answered value: Bool) {
        lock.lock()
        defer { lock.unlock() }
        guard !finished else { return }
        finished = true
        answered = value
        done.signal()
    }

    /// `true` only when a `hello_ack` arrived within `seconds`.
    func wait(seconds: Double) -> Bool {
        _ = done.wait(timeout: .now() + seconds)
        lock.lock()
        defer { lock.unlock() }
        finished = true
        return answered
    }
}
