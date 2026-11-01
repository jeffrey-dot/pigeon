import AppKit

/// When Pigeon looks for a new version and what it asks (`docs/update.md`): a quiet check
/// shortly after launch and once a day, plus Check for Updates… in the app menu. Nothing is
/// downloaded or installed until the user says so.
final class UpdateController: NSObject, NSMenuItemValidation {
    private let updater: Updater?
    private var timer: Timer?
    private var busy = false
    /// A version the user put off: the daily check does not ask about it again this run.
    private var postponed: String?
    let menuItem: NSMenuItem

    init(updater: Updater? = Updater.standard()) {
        self.updater = updater
        menuItem = NSMenuItem(
            title: "Check for Updates…", action: #selector(checkNow(_:)), keyEquivalent: "")
        super.init()
        menuItem.target = self
    }

    /// Starts the background checks; a build without a release version never checks.
    func start() {
        guard updater != nil else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 10) { [weak self] in
            self?.check(userAsked: false)
        }
        timer = Timer.scheduledTimer(withTimeInterval: 24 * 60 * 60, repeats: true) { [weak self] _ in
            self?.check(userAsked: false)
        }
    }

    @objc func checkNow(_ sender: Any?) {
        check(userAsked: true)
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        !busy
    }

    /// A background check stays silent unless there is something to install; one the user
    /// asked for always answers.
    private func check(userAsked: Bool) {
        guard !busy else { return }
        guard let updater else {
            if userAsked {
                inform(
                    "Updates aren't available for this copy of Pigeon.",
                    "It was built from source. Build the new version the same way.")
            }
            return
        }
        busy = true
        DispatchQueue.global(qos: .utility).async {
            let result = Result { try updater.check() }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                busy = false
                switch result {
                case let .success(manifest?):
                    if userAsked || manifest.version != postponed {
                        offer(manifest, updater: updater)
                    }
                case .success(nil):
                    if userAsked {
                        inform("You're up to date.", "Pigeon \(updater.current) is the latest version.")
                    }
                case .failure:
                    if userAsked {
                        inform("Couldn't check for updates.", "Make sure you're online, then try again.")
                    }
                }
            }
        }
    }

    private func offer(_ manifest: UpdateManifest, updater: Updater) {
        let alert = NSAlert()
        alert.messageText = "Pigeon \(manifest.version) is available"
        alert.informativeText = "You have \(updater.current). Pigeon will restart to finish; "
            + "your sessions keep running."
        alert.addButton(withTitle: "Install and Relaunch")
        alert.addButton(withTitle: "Later")
        guard alert.runModal() == .alertFirstButtonReturn else {
            postponed = manifest.version
            return
        }
        install(manifest, updater: updater)
    }

    private func install(_ manifest: UpdateManifest, updater: Updater) {
        busy = true
        menuItem.title = "Downloading Update…"
        let destination = Bundle.main.bundleURL
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result {
                let app = try updater.download(manifest, near: destination)
                try Updater.install(app, replacing: destination)
                try Updater.relaunch(destination, after: ProcessInfo.processInfo.processIdentifier)
            }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                switch result {
                case .success:
                    // The local server and its sessions are a separate process: quitting
                    // the window leaves them running for the new copy to reattach.
                    NSApp.terminate(nil)
                case let .failure(error):
                    busy = false
                    menuItem.title = "Check for Updates…"
                    inform("Couldn't update Pigeon.", "\(error)")
                }
            }
        }
    }

    private func inform(_ message: String, _ detail: String) {
        let alert = NSAlert()
        alert.messageText = message
        alert.informativeText = detail
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }
}
