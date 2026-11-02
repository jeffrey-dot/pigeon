import Foundation

/// The one hand-off point between the main thread and a pane's render thread.
///
/// The render thread polls it on each display refresh. The lock is held only to swap a value
/// in or out, never while building instances or drawing, so a slow frame cannot stall input
/// handling. A frame posted before the previous one was taken replaces it, its dirty rows
/// merged in, so the render thread always draws the newest screen and skips the ones it
/// could not keep up with.
final class FrameMailbox: @unchecked Sendable {
    private let lock = NSLock()
    private var pending: GridFrame?
    private var closed = false

    func post(_ frame: GridFrame) {
        lock.lock()
        if let older = pending {
            pending = frame.superseding(older)
        } else {
            pending = frame
        }
        lock.unlock()
    }

    /// The newest posted frame, or nil when none is waiting (or the mailbox is closed).
    func poll() -> GridFrame? {
        lock.lock()
        defer { lock.unlock() }
        guard !closed else { return nil }
        let frame = pending
        pending = nil
        return frame
    }

    /// Puts back a frame the render thread took but could not draw. A newer post wins; only
    /// the taken frame's dirty rows are carried over to it.
    func restore(_ frame: GridFrame) {
        lock.lock()
        if let newer = pending {
            pending = newer.superseding(frame)
        } else if !closed {
            pending = frame
        }
        lock.unlock()
    }

    var isClosed: Bool {
        lock.lock()
        defer { lock.unlock() }
        return closed
    }

    func close() {
        lock.lock()
        closed = true
        pending = nil
        lock.unlock()
    }
}
