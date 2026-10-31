import Foundation

extension AgentInfo {
    /// Reads a `status` message. `age_ms` is read as a u32 (49 days of milliseconds); a
    /// missing field, or an older host that does not send one, falls back to "just now".
    init(message: OpaquePointer, now: Date = Date()) {
        let absent = UInt32.max
        let age = Wire.u32(message, "age_ms", fallback: absent)
        self.init(
            app: Wire.string(message, "app"),
            message: Wire.string(message, "msg"),
            kind: Wire.string(message, "kind"),
            ageMs: age == absent ? nil : UInt64(age),
            now: now)
    }
}
