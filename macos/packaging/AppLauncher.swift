import AppKit
import Darwin

// Each Dock process controls one window lease; GlidePod coordinates guest input.
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let package: String
    private let socketPath: String
    private var fd: Int32 = -1
    private var reader: DispatchSourceRead?
    private var retry: Timer?
    private let deadline = Date().addingTimeInterval(180)
    private var received = Data()
    private var ready = false
    private var hostClosed = false
    private var wantsFocus = false
    private var wantsQuit = false

    init(package: String, socketPath: String) {
        self.package = package
        self.socketPath = socketPath
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let name = Bundle.main.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String ?? package
        if let icon = Bundle.main.url(forResource: "AppIcon", withExtension: "png") {
            NSApp.applicationIconImage = NSImage(contentsOf: icon)
        }
        let main = NSMenu()
        let item = NSMenuItem()
        let menu = NSMenu(title: name)
        let show = menu.addItem(withTitle: "Show " + name, action: #selector(showWindow), keyEquivalent: "")
        show.target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: "Quit " + name, action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        item.submenu = menu
        main.addItem(item)
        NSApp.mainMenu = main
        if !connectToWindow() {
            // A host-started entry must never reopen a window that was closed
            // before Launch Services finished starting this helper.
            if CommandLine.arguments.contains("--glidepod-managed") { finish(); return }
            launchHost()
        }
        retry = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            guard let self, !self.ready else { return }
            if Date() > self.deadline {
                self.fail("GlidePod could not open this app. Check the Android runtime in GlidePod and try again.")
            } else if self.fd < 0 {
                _ = self.connectToWindow()
            }
        }
    }

    private func connectToWindow() -> Bool {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = socketPath.utf8CString
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else {
            fail("The Dock connection path is too long."); return false
        }
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        withUnsafeMutableBytes(of: &address.sun_path) { target in
            bytes.withUnsafeBytes { target.copyBytes(from: $0) }
        }
        let socket = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard socket >= 0 else { return false }
        let result = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(socket, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else { Darwin.close(socket); return false }
        var suppress: Int32 = 1
        setsockopt(socket, SOL_SOCKET, SO_NOSIGPIPE, &suppress, socklen_t(MemoryLayout<Int32>.size))
        _ = fcntl(socket, F_SETFL, O_NONBLOCK)
        fd = socket
        let source = DispatchSource.makeReadSource(fileDescriptor: socket, queue: .main)
        source.setEventHandler { [weak self] in self?.readHost() }
        source.setCancelHandler { Darwin.close(socket) }
        reader = source
        source.resume()
        return true
    }

    private func readHost() {
        var bytes = [UInt8](repeating: 0, count: 64)
        let count = Darwin.read(fd, &bytes, bytes.count)
        if count == 0 { finish(); return }
        if count < 0 {
            if errno != EAGAIN && errno != EINTR { finish() }
            return
        }
        received.append(contentsOf: bytes.prefix(count))
        if received == Data("ready\n".utf8) {
            ready = true
            retry?.invalidate()
            if wantsQuit { send("quit"); finish() }
            else if wantsFocus { send("focus") }
        } else if received.count >= 6 { fail("Invalid Dock connection response.") }
    }

    private func launchHost() {
        let workspace = NSWorkspace.shared
        let saved = Bundle.main.object(forInfoDictionaryKey: "GlidePodHostBundle") as? String ?? ""
        let app = !saved.isEmpty && FileManager.default.fileExists(atPath: saved)
            ? URL(fileURLWithPath: saved)
            : workspace.urlForApplication(withBundleIdentifier: "dev.glidepod.desktop")
        guard let app, let url = URL(string: "glidepod://launch/" + package) else {
            fail("GlidePod is not installed. Open GlidePod once, then try again."); return
        }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = false
        if let data = Bundle.main.object(forInfoDictionaryKey: "GlidePodDataDirectory") as? String, !data.isEmpty {
            configuration.environment = ["GLIDEPOD_DATA_DIR": data]
        }
        workspace.open([url], withApplicationAt: app, configuration: configuration) { [weak self] _, error in
            if let error { DispatchQueue.main.async { self?.fail(error.localizedDescription) } }
        }
    }

    private func send(_ command: String) {
        guard ready, fd >= 0 else { return }
        let data = Array((command + "\n").utf8)
        let written = data.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }
        if written != data.count { finish() }
    }

    @objc func showWindow() { wantsFocus = true; send("focus") }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showWindow(); return false
    }

    func applicationDidBecomeActive(_ notification: Notification) { showWindow() }

    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        let menu = NSMenu()
        let show = menu.addItem(withTitle: "Show Window", action: #selector(showWindow), keyEquivalent: "")
        show.target = self
        return menu
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if hostClosed { return .terminateNow }
        if ready { send("quit"); return .terminateNow }
        wantsQuit = true
        return .terminateLater
    }

    private func finish() {
        hostClosed = true
        retry?.invalidate()
        reader?.cancel()
        reader = nil
        fd = -1
        if wantsQuit { NSApp.reply(toApplicationShouldTerminate: true) }
        else { NSApp.terminate(nil) }
    }

    private func fail(_ message: String) {
        retry?.invalidate()
        if !wantsQuit {
            let alert = NSAlert()
            alert.messageText = "Cannot open app"
            alert.informativeText = message
            alert.runModal()
        }
        finish()
    }
}

#if !DOCK_TESTS
guard let package = Bundle.main.object(forInfoDictionaryKey: "GlidePodPackage") as? String,
      let socket = Bundle.main.object(forInfoDictionaryKey: "GlidePodDockSocket") as? String,
      !package.isEmpty else { exit(1) }
let app = NSApplication.shared
let delegate = AppDelegate(package: package, socketPath: socket)
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
#endif
