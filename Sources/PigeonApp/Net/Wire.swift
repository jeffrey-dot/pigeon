import Foundation
import PigeonCore

/// Thin Swift skin over the C message codec. No CBOR lives here — the core owns
/// encoding, this file only moves values across the boundary.
enum Wire {
    static func make(_ type: PgMsgType) -> OpaquePointer? {
        pg_msg_new(type)
    }

    static func free(_ message: OpaquePointer?) {
        pg_msg_free(message)
    }

    static func type(_ message: OpaquePointer) -> PgMsgType {
        pg_msg_type(message)
    }

    static func set(_ message: OpaquePointer, u32 field: String, _ value: UInt32) {
        field.withCString { _ = pg_msg_set_u32(message, $0, value) }
    }

    static func set(_ message: OpaquePointer, i32 field: String, _ value: Int32) {
        field.withCString { _ = pg_msg_set_i32(message, $0, value) }
    }

    static func set(_ message: OpaquePointer, f32 field: String, _ value: Float) {
        field.withCString { _ = pg_msg_set_f32(message, $0, value) }
    }

    static func set(_ message: OpaquePointer, bool field: String, _ value: Bool) {
        field.withCString { _ = pg_msg_set_bool(message, $0, value) }
    }

    static func set(_ message: OpaquePointer, str field: String, _ value: String) {
        let bytes = Array(value.utf8)
        field.withCString { name in
            bytes.withUnsafeBufferPointer { buffer in
                let text = PgStr(
                    size: UInt32(MemoryLayout<PgStr>.size),
                    ptr: buffer.baseAddress,
                    len: buffer.count
                )
                _ = pg_msg_set_str(message, name, text)
            }
        }
    }

    static func set(_ message: OpaquePointer, u32Array field: String, _ values: [UInt32]) {
        field.withCString { name in
            values.withUnsafeBufferPointer { buffer in
                _ = pg_msg_set_u32_array(message, name, buffer.baseAddress, buffer.count)
            }
        }
    }

    static func set(_ message: OpaquePointer, bytes field: String, _ value: [UInt8]) {
        field.withCString { name in
            value.withUnsafeBufferPointer { buffer in
                _ = pg_msg_set_bytes(message, name, buffer.baseAddress, buffer.count)
            }
        }
    }

    static func set(_ message: OpaquePointer, strings field: String, _ values: [String]) {
        // One heap buffer per string, freed after the call: the C side copies.
        var buffers: [UnsafeMutablePointer<UInt8>] = []
        defer { buffers.forEach { $0.deallocate() } }

        var table: [PgStr] = []
        table.reserveCapacity(values.count)
        for value in values {
            let bytes = Array(value.utf8)
            let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: max(bytes.count, 1))
            if !bytes.isEmpty {
                bytes.withUnsafeBufferPointer { buffer.update(from: $0.baseAddress!, count: bytes.count) }
            }
            buffers.append(buffer)
            table.append(
                PgStr(
                    size: UInt32(MemoryLayout<PgStr>.size),
                    ptr: UnsafePointer(buffer),
                    len: bytes.count
                )
            )
        }

        field.withCString { name in
            table.withUnsafeBufferPointer { buffer in
                _ = pg_msg_set_str_array(message, name, buffer.baseAddress, buffer.count)
            }
        }
    }

    /// Whether a string-array field (`hello_ack.features`) contains `value`.
    static func contains(_ message: OpaquePointer, strings field: String, _ value: String) -> Bool {
        var bytes = Array(value.utf8)
        return bytes.withUnsafeMutableBufferPointer { buffer in
            let text = PgStr(
                size: UInt32(MemoryLayout<PgStr>.size),
                ptr: UnsafePointer(buffer.baseAddress), len: buffer.count)
            return field.withCString { pg_msg_str_array_contains(message, $0, text) }
        }
    }

    static func u32(_ message: OpaquePointer, _ field: String, fallback: UInt32 = 0) -> UInt32 {
        field.withCString { pg_msg_get_u32(message, $0, fallback) }
    }

    static func i32(_ message: OpaquePointer, _ field: String, fallback: Int32 = 0) -> Int32 {
        field.withCString { pg_msg_get_i32(message, $0, fallback) }
    }

    static func bool(_ message: OpaquePointer, _ field: String, fallback: Bool = false) -> Bool {
        field.withCString { pg_msg_get_bool(message, $0, fallback) }
    }

    static func string(_ message: OpaquePointer, _ field: String) -> String {
        field.withCString { name in
            let text = pg_msg_get_str(message, name)
            guard let pointer = text.ptr, text.len > 0 else { return "" }
            return String(decoding: UnsafeBufferPointer(start: pointer, count: text.len), as: UTF8.self)
        }
    }

    static func string(_ text: PgStr) -> String {
        guard let pointer = text.ptr, text.len > 0 else { return "" }
        return String(decoding: UnsafeBufferPointer(start: pointer, count: text.len), as: UTF8.self)
    }

    static func frame(_ message: OpaquePointer) -> Data? {
        let length = pg_msg_encoded_len(message)
        guard length > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: length)
        let written = bytes.withUnsafeMutableBufferPointer { buffer in
            pg_msg_encode_frame(message, buffer.baseAddress, buffer.count)
        }
        guard written == length else { return nil }
        return Data(bytes)
    }
}
