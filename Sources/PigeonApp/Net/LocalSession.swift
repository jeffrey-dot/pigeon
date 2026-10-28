import Foundation
import PigeonCore

/// Degraded remote sessions (`docs/remote.md` §1 C): there is no `pigeon-host` on the far
/// side, so this window owns the VT state and feeds it the bytes of a plain `ssh -tt` session.
///
/// The trade-off is the whole point of mode C: the program runs, `OSC 7501` status still
/// arrives, but the session dies with this window.
///
/// shortcut: the remote pty keeps the size ssh allocated it (80×24) because the client has no
/// local pty to forward `TIOCSWINSZ` from; give the ssh child a pty when degraded mode matters
/// more than a few lines of `forkpty`.
final class LocalSession {
    private let core: OpaquePointer
    let pane: UInt32
    /// Called on the session's serial queue after the screen changed; hop to main in it.
    var onUpdate: (() -> Void)?

    private let queue = DispatchQueue(label: "pigeon.local.session")
    private var stdin: FileHandle?

    init() {
        core = pg_core_new(nil)
        pane = pg_pane_new(core, nil)
    }

    deinit {
        pg_core_free(core)
    }

    /// Starts reading the ssh pty and writing to it (keys and the core's DA/DSR answers).
    func attach(process: Process) {
        guard let ends = RemoteSession.pipeEnds(process) else { return }
        stdin = ends.input
        let output = ends.output
        let queue = self.queue
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            while let self {
                let data = output.readData(ofLength: 16 * 1024)
                if data.isEmpty { break }
                queue.async {
                    Self.feed(self.core, self.pane, data)
                    self.flushReplies()
                    // Runs on the serial queue: reading the snapshot from here would
                    // deadlock, so a caller that wants one hops to another queue first.
                    self.onUpdate?()
                }
            }
        }
    }

    func write(_ bytes: [UInt8]) {
        stdin?.write(Data(bytes))
    }

    /// Feeds bytes straight into the VT — the same path the ssh reader takes.
    func feed(_ bytes: [UInt8]) {
        queue.sync { Self.feed(self.core, self.pane, Data(bytes)) }
    }

    /// The screen as the ABI sees it; the caller frees it.
    func snapshot() -> UnsafeMutablePointer<PgSnapshot>? {
        queue.sync { pg_pane_snapshot(self.core, self.pane) }
    }

    /// Same, without hopping onto the serial queue — only legal from `onUpdate`, which
    /// already runs there (`snapshot()` would deadlock).
    func snapshotUnsafeFromQueue() -> UnsafeMutablePointer<PgSnapshot>? {
        pg_pane_snapshot(core, pane)
    }

    func resize(cols: UInt16, rows: UInt16) {
        queue.sync { _ = pg_pane_resize(self.core, self.pane, cols, rows) }
    }

    private func flushReplies() {
        var buffer = [UInt8](repeating: 0, count: 4096)
        let count = buffer.withUnsafeMutableBufferPointer { raw in
            pg_pane_respond(core, pane, raw.baseAddress, raw.count)
        }
        guard count > 0 else { return }
        stdin?.write(Data(buffer[0..<count]))
    }

    @discardableResult
    private static func feed(_ core: OpaquePointer, _ pane: UInt32, _ data: Data) -> Bool {
        return data.withUnsafeBytes { raw in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else { return false }
            return pg_pane_feed(core, pane, base, data.count)
        }
    }
}
