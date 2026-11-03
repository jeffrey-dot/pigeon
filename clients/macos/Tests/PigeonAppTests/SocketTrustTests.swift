import Darwin
import XCTest
@testable import PigeonApp

/// The client refuses a socket another user could have planted or swapped.
final class SocketTrustTests: XCTestCase {
    /// Under `/tmp`: the per-user `$TMPDIR` is too long for `sun_path`.
    private func scratch() throws -> String {
        let path = "/tmp/pigeon-trust-\(getpid())-\(UUID().uuidString.prefix(8))"
        XCTAssertEqual(mkdir(path, 0o700), 0)
        XCTAssertEqual(chmod(path, 0o700), 0)
        return path
    }

    private func bindSocket(_ path: String) -> Int32 {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: Array(path.utf8))
        }
        let size = socklen_t(MemoryLayout<sockaddr_un>.size)
        let bound = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.bind(fd, $0, size) }
        }
        XCTAssertEqual(bound, 0)
        XCTAssertEqual(listen(fd, 1), 0)
        return fd
    }

    func testTheDefaultPathIsInAPrivateDirectory() {
        XCTAssertEqual(SocketTrust.defaultPath(environment: [:], uid: 501), "/tmp/pigeon-501/pigeon.sock")
        XCTAssertEqual(
            SocketTrust.defaultPath(environment: ["XDG_RUNTIME_DIR": "/run/user/501"], uid: 501),
            "/run/user/501/pigeon.sock")
    }

    func testOurSocketInAPrivateDirectoryIsTrusted() throws {
        let dir = try scratch()
        let path = dir + "/pigeon.sock"
        let listener = bindSocket(path)
        defer { Darwin.close(listener); unlink(path); rmdir(dir) }
        XCTAssertNoThrow(try SocketTrust.checkPath(path, uid: getuid()))
        XCTAssertThrowsError(try SocketTrust.checkPath(path, uid: getuid() + 1), "someone else's socket")
        // The peer of a connection to our own listener is us.
        let client = try HostClient.connect(path: path)
        client.close()
    }

    func testAWritableDirectoryOrANonSocketIsRefused() throws {
        let dir = try scratch()
        let path = dir + "/pigeon.sock"
        defer { unlink(path); chmod(dir, 0o700); rmdir(dir) }
        FileManager.default.createFile(atPath: path, contents: Data("x".utf8))
        XCTAssertThrowsError(try SocketTrust.checkPath(path, uid: getuid()), "a regular file")
        unlink(path)
        let listener = bindSocket(path)
        defer { Darwin.close(listener) }
        XCTAssertEqual(chmod(dir, 0o777), 0)
        XCTAssertThrowsError(try SocketTrust.checkPath(path, uid: getuid()), "a world-writable directory")
        XCTAssertThrowsError(try HostClient.connect(path: path))
    }

    func testASymlinkedSocketIsRefused() throws {
        let dir = try scratch()
        let real = dir + "/real.sock"
        let link = dir + "/pigeon.sock"
        let listener = bindSocket(real)
        defer { Darwin.close(listener); unlink(link); unlink(real); rmdir(dir) }
        XCTAssertEqual(symlink(real, link), 0)
        XCTAssertThrowsError(try SocketTrust.checkPath(link, uid: getuid()))
    }
}
