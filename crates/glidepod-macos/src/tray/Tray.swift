import AppKit
import SwiftUI

@MainActor
final class GlidePodTray: NSObject, NSPopoverDelegate {
    private(set) var item: NSStatusItem?
    let popover = NSPopover()
    let model = TrayModel()
    private(set) var timer: Timer?
    private var actions: [TrayAction] = []

    init(icon: NSImage) {
        super.init()
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        self.item = item
        item.button?.image = trayStatusIcon()
        item.button?.setAccessibilityLabel("GlidePod")
        item.button?.target = self
        item.button?.action = #selector(toggle)
        item.button?.sendAction(on: [.leftMouseUp, .rightMouseUp])
        popover.behavior = .transient
        popover.delegate = self
        popover.contentViewController = NSHostingController(rootView: TrayPanel(
            model: model, icon: icon,
            select: { [weak self] in self?.select($0) },
            dismiss: { [weak self] in self?.popover.performClose(nil) }))
        refresh()
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    func refresh() {
        model.refresh(TrayMailbox.shared.current(), preservingApps: popover.isShown)
        item?.button?.toolTip = "GlidePod · \(model.snapshot.state) · Open apps: \(model.snapshot.app_count.map(String.init) ?? "—")"
    }

    @objc func toggle() {
        if popover.isShown { popover.performClose(nil) } else { show() }
    }

    func show() {
        guard let button = item?.button, !popover.isShown else { return }
        refresh()
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        popover.contentViewController?.view.window?.makeKey()
    }

    func popoverDidClose(_ notification: Notification) {
        refresh()
    }

    func select(_ action: TrayAction) {
        guard item != nil, actions.count < 32 else { return }
        if case .focusApp(let package) = action,
           !TrayMailbox.shared.current().apps.contains(where: { $0.id == package }) { return }
        actions.append(action)
        popover.performClose(nil)
    }

    func drain() -> String? {
        let data = try? JSONSerialization.data(withJSONObject: actions.map(\.payload), options: [.sortedKeys])
        actions.removeAll(keepingCapacity: true)
        return data.flatMap { String(data: $0, encoding: .utf8) }
    }

    func dispose() {
        timer?.invalidate()
        timer = nil
        popover.close()
        popover.delegate = nil
        popover.contentViewController = nil
        if let item { NSStatusBar.system.removeStatusItem(item) }
        item = nil
    }
}

@MainActor
func trayStatusIcon() -> NSImage {
    // A small vector version of the two package faces, without the app-icon
    // background or glass effects. AppKit supplies the menu bar tint.
    let image = NSImage(size: NSSize(width: 18, height: 18), flipped: true) { _ in
        func rotation(around center: NSPoint, degrees: CGFloat) -> AffineTransform {
            var transform = AffineTransform()
            transform.translate(x: center.x, y: center.y)
            transform.rotate(byDegrees: degrees)
            transform.translate(x: -center.x, y: -center.y)
            return transform
        }
        let rear = NSBezierPath(roundedRect: NSRect(x: 2, y: 1.75, width: 10.5, height: 10.5),
                                xRadius: 2.25, yRadius: 2.25)
        rear.transform(using: rotation(around: NSPoint(x: 7.25, y: 7), degrees: -12))
        NSColor.black.setStroke()
        rear.lineWidth = 1.5
        rear.stroke()

        let frontRotation = rotation(around: NSPoint(x: 10.75, y: 11), degrees: 12)
        let front = NSBezierPath(roundedRect: NSRect(x: 5.5, y: 5.75, width: 10.5, height: 10.5),
                                 xRadius: 2.25, yRadius: 2.25)
        front.transform(using: frontRotation)
        // Knock out the hidden rear contour while keeping the face transparent.
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current?.compositingOperation = .clear
        front.fill()
        NSGraphicsContext.restoreGraphicsState()
        front.lineWidth = 1.5
        front.stroke()

        let slot = NSBezierPath()
        slot.move(to: NSPoint(x: 8, y: 8.75))
        slot.line(to: NSPoint(x: 11.75, y: 8.75))
        slot.transform(using: frontRotation)
        slot.lineWidth = 1.5
        slot.lineCapStyle = .round
        slot.stroke()
        return true
    }
    image.isTemplate = true
    image.accessibilityDescription = "GlidePod"
    return image
}

@MainActor
func trayIcon(fallback: Data, bundle: Bundle = .main) -> NSImage? {
    // The panel header keeps the layered application icon, with the complete
    // ICNS as its fallback. Direct cargo runs use the same exported artwork.
    var bundles = [bundle]
    // The windowless background agent lives in Contents/Library/LoginItems.
    // Read the desktop's catalog there too, without duplicating icon assets.
    if bundle.bundleIdentifier == "dev.glidepod.background" {
        let parent = bundle.bundleURL.deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent()
        if let desktop = Bundle(url: parent), desktop.bundleIdentifier == "dev.glidepod.desktop" {
            bundles.append(desktop)
        }
    }
    for bundle in bundles {
        let name = bundle.object(forInfoDictionaryKey: "CFBundleIconName") as? String ?? "GlidePod"
        if let image = bundle.image(forResource: NSImage.Name(name))
            ?? bundle.url(forResource: "GlidePod", withExtension: "icns").flatMap({ NSImage(contentsOf: $0) }) {
            return image
        }
    }
    return NSImage(data: fallback)
}

@_cdecl("glidepod_tray_new")
@MainActor
func glidepodTrayNew(_ data: UnsafePointer<UInt8>?, _ length: Int) -> UnsafeMutableRawPointer? {
    guard Thread.isMainThread, let data, length > 0 else { return nil }
    guard let icon = trayIcon(fallback: Data(bytes: data, count: length)) else { return nil }
    return Unmanaged.passRetained(GlidePodTray(icon: icon)).toOpaque()
}

@_cdecl("glidepod_tray_free")
@MainActor
func glidepodTrayFree(_ handle: UnsafeMutableRawPointer?) {
    precondition(Thread.isMainThread)
    guard let handle else { return }
    Unmanaged<GlidePodTray>.fromOpaque(handle).takeRetainedValue().dispose()
}

@_cdecl("glidepod_tray_show")
@MainActor
func glidepodTrayShow(_ handle: UnsafeMutableRawPointer?) {
    precondition(Thread.isMainThread)
    guard let handle else { return }
    let tray = Unmanaged<GlidePodTray>.fromOpaque(handle).takeUnretainedValue()
    // Leave GPUI's action callback before creating an AppKit popup window.
    DispatchQueue.main.async { [weak tray] in tray?.show() }
}

@_cdecl("glidepod_tray_publish")
func glidepodTrayPublish(_ data: UnsafePointer<UInt8>?, _ length: Int) {
    guard let data, length > 0, length <= 1_048_576 else { return }
    TrayMailbox.shared.publish(Data(bytes: data, count: length))
}

@_cdecl("glidepod_tray_drain")
@MainActor
func glidepodTrayDrain(_ handle: UnsafeMutableRawPointer?) -> UnsafeMutablePointer<CChar>? {
    precondition(Thread.isMainThread)
    guard let handle else { return nil }
    return Unmanaged<GlidePodTray>.fromOpaque(handle).takeUnretainedValue().drain().flatMap { strdup($0) }
}
