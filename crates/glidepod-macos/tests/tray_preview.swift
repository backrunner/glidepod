import AppKit

// An isolated fixture host for the real status item/popover and action queue.
// It never connects to Rust or starts an Android VM.
@MainActor
private final class PreviewDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow!
    private var tray: GlidePodTray!
    private var timer: Timer?
    private let result = NSTextField(labelWithString: "Fixture data · No action selected")

    func applicationDidFinishLaunching(_ notification: Notification) {
        let bundle = Bundle.main
        let data = try! Data(contentsOf: bundle.url(forResource: "fixture", withExtension: "json")!)
        tray = GlidePodTray(icon: bundle.image(forResource: "GlidePod")!)
        DispatchQueue.global(qos: .utility).async { TrayMailbox.shared.publish(data) }
        tray.refresh()
        let show = NSButton(title: "Open Tray Preview", target: self, action: #selector(openTray))
        let stack = NSStackView(views: [show, result])
        stack.orientation = .vertical
        stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 560, height: 120),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "GlidePod Tray — Fixture Preview"
        window.contentView = stack
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        timer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                DispatchQueue.global(qos: .utility).async { TrayMailbox.shared.publish(data) }
                if let actions = self.tray.drain(), actions != "[]" { self.result.stringValue = actions }
            }
        }
    }

    @objc private func openTray() { tray.show() }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main
struct TrayPreview {
    @MainActor
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = PreviewDelegate()
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}
