import Darwin
import Foundation
import PigeonCore

/// A unix-socket client. Framing and encoding are delegated to the core's codec;
/// this type only moves bytes.
final class HostClient {
    enum Failure: Error, CustomStringConvertible {
        case connect(String)
        case closed

        var description: String {
            switch self {
            case let .connect(detail): return "connect: \(detail)"
            case .closed: return "host closed the connection"
            }
        }
    }

    private let fd: Int32
    /// Where frames go. Equal to `fd` for a socket; a separate pipe for `ssh ... --proxy`.
    private let writeFD: Int32
    private let source: DispatchSourceRead
    private var inbox = Data()

    /// Receives an owned message; the callee must `Wire.free` it.
    var onMessage: ((OpaquePointer) -> Void)?
    var onClose: (() -> Void)?

    static func connect(path: String) throws -> HostClient {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard pathBytes.count < capacity else {
            throw Failure.connect("socket path too long")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: pathBytes)
        }

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw Failure.connect(String(cString: strerror(errno))) }

        let size = socklen_t(MemoryLayout<sockaddr_un>.size)
        let result = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, size)
            }
        }
        guard result == 0 else {
            let detail = String(cString: strerror(errno))
            Darwin.close(fd)
            throw Failure.connect(detail)
        }

        return HostClient(read: fd, write: fd)
    }

    /// Wraps two pipes: what a child process (`ssh ... --proxy`) gives us is not one fd.
    init(read: Int32, write: Int32) {
        self.fd = read
        writeFD = write
        source = DispatchSource.makeReadSource(fileDescriptor: read, queue: .global(qos: .userInitiated))
        source.setEventHandler { [weak self] in self?.readAvailable() }
        source.setCancelHandler {
            Darwin.close(read)
            if write != read { Darwin.close(write) }
        }
        source.resume()
    }

    func send(_ message: OpaquePointer) {
        guard let frame = Wire.frame(message) else {
            NSLog("pigeon: refusing to send an incomplete message type=\(Wire.type(message).rawValue)")
            return
        }
        frame.withUnsafeBytes { raw in
            var offset = 0
            while offset < raw.count {
                let written = Darwin.write(writeFD, raw.baseAddress!.advanced(by: offset), raw.count - offset)
                if written <= 0 { break }
                offset += written
            }
        }
    }

    func close() {
        source.cancel()
    }

    private func readAvailable() {
        var chunk = [UInt8](repeating: 0, count: 16 * 1024)
        let count = Darwin.read(fd, &chunk, chunk.count)
        if count <= 0 {
            onClose?()
            close()
            return
        }
        inbox.append(contentsOf: chunk[0..<count])
        drain()
    }

    private func drain() {
        while true {
            var consumed = 0
            var message: OpaquePointer?
            inbox.withUnsafeBytes { raw in
                message = pg_msg_decode_frame(
                    raw.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    raw.count,
                    &consumed
                )
            }
            if let message {
                inbox.removeFirst(consumed)
                onMessage?(message)
                continue
            }
            if pg_last_error_code() != PG_OK {
                NSLog("pigeon: protocol error")
                onClose?()
                close()
            }
            return
        }
    }
}
