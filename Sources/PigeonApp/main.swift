import AppKit

// Every socket and pipe write reports a vanished host as EPIPE; the default SIGPIPE would
// kill the app instead.
signal(SIGPIPE, SIG_IGN)

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
