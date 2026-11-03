/// One session on one machine. Session ids restart at 1 on every host, so the pair is the
/// identity; the sidebar and the persisted state key on it instead of packing both into an Int.
struct SessionRef: Hashable, Codable {
    var host: String
    var session: Int

    /// The persisted key (`sidebar.json`): this Mac keeps the bare session number older
    /// builds wrote, a remote host is `host:session`.
    var key: String {
        host == "local" ? String(session) : "\(host):\(session)"
    }

    init(host: String, session: Int) {
        self.host = host
        self.session = session
    }

    /// The inverse of `key`. Host names may contain `:` (`[::1]`, `user@host:2222`), so the
    /// session is what follows the *last* colon. Anything that is not a positive session
    /// number is refused.
    init?(key: String) {
        guard let colon = key.lastIndex(of: ":") else {
            guard let session = Int(key), session > 0 else { return nil }
            self.init(host: "local", session: session)
            return
        }
        let host = String(key[..<colon])
        guard !host.isEmpty, let session = Int(key[key.index(after: colon)...]), session > 0
        else { return nil }
        self.init(host: host, session: session)
    }
}

/// Hands out the Int ids the sidebar's rows and drag pasteboard use. An id names one
/// `SessionRef` for the whole run, whatever happens to the host list, and is never reused.
struct SessionRefs {
    private var ids: [SessionRef: Int] = [:]
    private var refs: [Int: SessionRef] = [:]
    private var next = 1

    mutating func id(for ref: SessionRef) -> Int {
        if let id = ids[ref] { return id }
        let id = next
        next += 1
        ids[ref] = id
        refs[id] = ref
        return id
    }

    func ref(for id: Int) -> SessionRef? { refs[id] }

    /// An id seen before, without minting one (lookups of a row that may be gone).
    func existingID(for ref: SessionRef) -> Int? { ids[ref] }
}
