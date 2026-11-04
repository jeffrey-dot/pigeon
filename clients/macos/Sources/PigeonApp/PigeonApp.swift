import AppKit
import PigeonCore

/// Wires the window to a `pigeon-server`: sidebar on the left, split container on
/// the right, and the protocol messages that keep both in sync.
final class AppDelegate: NSObject, NSApplicationDelegate {
    typealias Row = SessionRow

    var window: NSWindow!
    var splitView: NSSplitView!
    var sidebar: SidebarViewController!
    var header: SessionHeaderView!
    var container: SplitContainerView!
    var hostStatus: HostStatusView!
    /// One live connection per machine (`docs/remote.md` §4). `"local"` is this Mac.
    var clients: [String: HostClient] = [:]
    var remotes: [String: RemoteSession] = [:]
    /// Per-machine state (`HostConnection`): rows, screens on display, seq, liveness.
    var connections: [String: HostConnection<Row>] = [:]
    var heartbeatTimer: Timer?
    var recovering: [String: Int] = [:]
    private let hostAgent = HostAgent()
    let updates = UpdateController()
    /// Set by "End All Sessions on This Mac…" so the dropped socket is not taken for a crash.
    var stoppedByUser = false
    /// The host whose layout is on screen; every host stays connected in the background.
    var activeHost = "local"
    /// The layout actually on screen can lag behind a session/host selection.
    var displayedHost: String?
    var displayedSession: Int?
    /// Stable order for namespaced sidebar ids (`host index * 100_000 + session`).
    var hostOrder: [String] = ["local"]

    var client: HostClient? {
        get { clients[activeHost] }
        set { clients[activeHost] = newValue }
    }
    var remote: RemoteSession? {
        get { remotes[activeHost] }
        set { remotes[activeHost] = newValue }
    }
    /// The active host's connection state; reading a host never seen yields an empty one.
    var connection: HostConnection<Row> {
        get { connections[activeHost] ?? HostConnection() }
        set { connections[activeHost] = newValue }
    }
    var rows: [Int: Row] {
        get { connection.rows }
        set { connection.rows = newValue }
    }
    var focusedPane: UInt32? {
        get { connection.focusedPane }
        set { connection.focusedPane = newValue }
    }
    var haveSnapshot: Set<UInt32> {
        get { connection.seen }
        set { connection.seen = newValue }
    }
    var requested: Set<UInt32> {
        get { connection.requested }
        set { connection.requested = newValue }
    }
    var pendingSnapshots: [UInt32: Data] {
        get { connection.pending }
        set { connection.pending = newValue }
    }
    var frameSeq: [UInt32: UInt32] {
        get { connection.seq }
        set { connection.seq = newValue }
    }
    /// Mode C keeps the VT state here (no host on the far side).
    var localSession: LocalSession?
    /// Why the host rejected this client (bad version, busy); retrying cannot help.
    var refusedReason: String?
    /// Messages this window has sent, by type: what the self-test asserts on.
    var sentCounts: [PgMsgType: Int] = [:]
    /// `PIGEON_SELFTEST=1` drives the real menu/keyboard/sidebar paths once and exits.
    let selfTest = ProcessInfo.processInfo.environment["PIGEON_SELFTEST"] == "1"
    var selfTestRunning = false
    /// The last row the sidebar asked to select (a `Machines` row uses a negative id).
    var lastSelectedId: Int?
    var sessionRefs = SessionRefs()

    var selectedSession: Int? {
        get { connection.selectedSession }
        set { connection.selectedSession = newValue }
    }
    var nextRequest: UInt32 = 1
    /// Sessions per host, and which host is the one on screen. Every host keeps its own
    /// connection (`docs/remote.md` §4: one ssh per machine, N sessions on it).
    /// Session created by `+` / ⌘T: select it once the refreshed list contains it.
    var pendingSelectSession: Int?
    /// Hosts whose first session list has arrived. Only that first list may start a session
    /// on its own: a later empty list means the user closed them all.
    var listedHosts: Set<String> = []

    private var appearanceObservation: NSKeyValueObservation?
    let store = SidebarStore()
    var state = SidebarState()
    let hostsStore = HostsStore()
    let settingsStore = TerminalSettingsStore()
    lazy var settings = settingsStore.load()
    var hosts = HostsState()
    /// Set while a remote probe runs, so the sidebar row says `connecting` rather than
    /// `offline` (docs/remote.md §4 asks for the status on the host, not only in a banner).
    var connectingHost: String?
    /// The visible host while a background host's message is being handled (`activeHost`
    /// then points at the background one). Sidebar redraws wait until it is restored.
    var swappedFrom: String?
    var sidebarDirty = false
    /// "Jeffrey's MacBook Air"; looked up once because the lookup can touch the resolver.
    lazy var localMachineName: String = Host.current().localizedName ?? ""

    func applicationDidFinishLaunching(_ notification: Notification) {
        // `PIGEON_APPEARANCE=light|dark|system` pins the app's appearance for the screenshot
        // runs (`tools/checks/m3-gui-check.sh`). Pinning sets `NSApp.appearance`, which overrides the
        // system until it is cleared again — hence the third value and the three-way toggle.
        NSApp.appearance = AppearanceMode
            .fromEnvironment(ProcessInfo.processInfo.environment["PIGEON_APPEARANCE"]).appearance
        // The system flipping light/dark is another way the palette changes. AppKit does not
        // post a notification for it, but `effectiveAppearance` is KVO-observable.
        appearanceObservation = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
            self?.sendAppearance()
        }
        // Older builds registered a launchd job, which macOS lists as a background item;
        // the host now starts like a tmux server instead. Tests never touch the real one.
        if ProcessInfo.processInfo.environment["PIGEON_CONFIG_DIR"] == nil, hostAgent.removeLegacyJob() {
            debugLog("removed the legacy launchd job \(HostAgent.defaultLabel)")
        }
        state = store.load()
        hosts = hostsStore.load()
        buildMenu()
        buildWindow()
        updates.start()
        if let host = ProcessInfo.processInfo.environment["PIGEON_REMOTE"], !host.isEmpty {
            startRemote(host, select: true)
            return
        }
        // Every known machine connects at launch (`docs/remote.md` §4): the sidebar lists
        // their sessions side by side, not one at a time.
        activeHost = hosts.current ?? "local"
        if !connectHost() {
            hostLost(host: "local", "Pigeon's background service isn't running.")
        }
        for host in hosts.hosts {
            startRemote(host)
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    /// Writes leave on a background queue; a quit right after `close_pane` must not drop it.
    func applicationWillTerminate(_ notification: Notification) {
        for client in clients.values { client.flushWrites() }
    }
}
