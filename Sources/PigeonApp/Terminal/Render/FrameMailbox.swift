import Foundation

/// The one hand-off point between the main thread and a pane's render thread.
///
/// The lock is held only to swap a value in or out, never while building instances or
/// drawing, so a slow frame cannot stall input handling. A frame posted before the previous
/// one was taken replaces it, its dirty rows merged in, so the render thread always draws
/// the newest screen and skips the ones it could not keep up with.
final class FrameMailbox: @unchecked Sendable {
    private let condition = NSCondition()
    private var pending: GridFrame?
    private var closed = false

    func post(_ frame: GridFrame) {
        condition.lock()
        if let older = pending {
            pending = frame.superseding(older)
        } else {
            pending = frame
        }
        condition.signal()
        condition.unlock()
    }

    /// Blocks until a frame arrives; nil once the mailbox is closed.
    func take() -> GridFrame? {
        condition.lock()
        defer { condition.unlock() }
        while pending == nil, !closed {
            condition.wait()
        }
        guard !closed else { return nil }
        let frame = pending
        pending = nil
        return frame
    }

    /// Non-blocking take, for tests.
    func poll() -> GridFrame? {
        condition.lock()
        defer { condition.unlock() }
        let frame = pending
        pending = nil
        return frame
    }

    func close() {
        condition.lock()
        closed = true
        condition.broadcast()
        condition.unlock()
    }
}
