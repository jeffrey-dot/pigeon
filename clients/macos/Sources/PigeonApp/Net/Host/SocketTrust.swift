import Darwin
import Foundation

/// The client half of the host's socket rules (`core/src/bin/pigeon-server/socket_trust.rs`):
/// before connecting, the socket's directory must be ours and not writable by others (or
/// root's and sticky, like `/tmp`) and the socket itself a socket owned by us, both checked
/// with `lstat` so no link is followed; after connecting, the peer must run as us
/// (`getpeereid`). Otherwise another local user could stand up a fake host and read every
/// keystroke.
enum SocketTrust {
    enum Failure: Error, CustomStringConvertible, Equatable {
        case untrusted(String)

        var description: String {
            switch self {
            case let .untrusted(detail): return detail
            }
        }
    }

    /// `/tmp/pigeon-<uid>/pigeon.sock` unless `XDG_RUNTIME_DIR` is set; the same rule as the
    /// host's `--socket-path`.
    static func defaultPath(environment: [String: String], uid: uid_t) -> String {
        if let runtime = environment["XDG_RUNTIME_DIR"] {
            return runtime + "/pigeon.sock"
        }
        return "/tmp/pigeon-\(uid)/pigeon.sock"
    }

    static func checkPath(_ path: String, uid: uid_t) throws {
        let directory = (path as NSString).deletingLastPathComponent
        try checkDirectory(directory.isEmpty ? "." : directory, uid: uid)
        var info = stat()
        guard lstat(path, &info) == 0 else {
            throw Failure.untrusted("\(path): \(String(cString: strerror(errno)))")
        }
        guard info.st_mode & S_IFMT == S_IFSOCK else {
            throw Failure.untrusted("\(path) is not a socket")
        }
        guard info.st_uid == uid else {
            throw Failure.untrusted("\(path) belongs to uid \(info.st_uid)")
        }
    }

    /// The directory is resolved first (`/tmp` is a link to `/private/tmp`); a link to
    /// someone else's directory then fails the owner check.
    static func checkDirectory(_ directory: String, uid: uid_t) throws {
        guard let resolved = realpath(directory, nil) else {
            throw Failure.untrusted("\(directory): \(String(cString: strerror(errno)))")
        }
        defer { free(resolved) }
        var info = stat()
        guard lstat(resolved, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else {
            throw Failure.untrusted("\(directory) is not a directory")
        }
        let ours = info.st_uid == uid && info.st_mode & 0o022 == 0
        let sharedSticky = info.st_uid == 0 && info.st_mode & S_ISVTX != 0
        guard ours || sharedSticky else {
            throw Failure.untrusted(
                "\(directory) is owned by uid \(info.st_uid) with mode \(String(info.st_mode & 0o7777, radix: 8))")
        }
    }

    static func checkPeer(_ fd: Int32, uid: uid_t) throws {
        var peerUID: uid_t = 0
        var peerGID: gid_t = 0
        guard getpeereid(fd, &peerUID, &peerGID) == 0 else {
            throw Failure.untrusted("peer credentials: \(String(cString: strerror(errno)))")
        }
        guard peerUID == uid else {
            throw Failure.untrusted("the host runs as uid \(peerUID), not \(uid)")
        }
    }
}
