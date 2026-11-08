import AppKit
import SwiftUI

private func require(_ condition: @autoclosure () -> Bool, _ message: String) {
    precondition(condition(), message)
}

private func sample(state: String = "Running", cpu: String = "142.5%", apps: Bool = true,
                    validFor: Double = 5) -> Data {
    try! JSONSerialization.data(withJSONObject: [
        "state": state, "detail": "Ready", "cpu": cpu, "memory": "2.50 GiB",
        "allocation": "4 vCPUs · 4.00 GiB", "app_count": apps ? 2 : 0,
        "apps": apps ? [["title": "时钟", "package_id": "test.clock"],
                        ["title": "Settings", "package_id": "test.settings"]] : [],
        "valid_for_seconds": validFor,
    ])
}

private func publish(_ data: Data) {
    data.withUnsafeBytes { glidepodTrayPublish($0.bindMemory(to: UInt8.self).baseAddress, data.count) }
}

private func publishFromWorker(_ data: Data, mailbox: TrayMailbox? = nil) {
    let done = DispatchSemaphore(value: 0)
    DispatchQueue.global(qos: .utility).async {
        require(!Thread.isMainThread, "Publication test must use a real worker thread")
        if let mailbox { mailbox.publish(data) } else { publish(data) }
        done.signal()
    }
    require(done.wait(timeout: .now() + 3) == .success, "Worker publication timed out")
}

@main
struct TrayNativeChecks {
    @MainActor
    static func main() throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.accessory)
        NSApp.finishLaunching()
        let iconData = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        try checkBackgroundIcon(iconData)
        let smallIconURL = URL(fileURLWithPath: CommandLine.arguments[1]).deletingLastPathComponent()
            .appendingPathComponent("glidepod-sidebar-light@1x.png")
        try checkAppIcons(Data(contentsOf: smallIconURL))
        checkFreshness()
        let handle = iconData.withUnsafeBytes {
            glidepodTrayNew($0.bindMemory(to: UInt8.self).baseAddress, iconData.count)
        }!
        let tray = Unmanaged<GlidePodTray>.fromOpaque(handle).takeUnretainedValue()
        // This callback harness pumps RunLoop, not NSApplication.run's animation
        // transaction loop. Exercise show/close synchronously here.
        tray.popover.animates = false
        let button = tray.item!.button!
        require(button.image != nil && !button.image!.isTemplate, "Branded color icon missing")
        require(button.image!.size == NSSize(width: 20, height: 20), "Wrong status icon size")
        require(button.image!.representations.contains { $0.pixelsWide >= 40 }, "No Retina icon")
        require(tray.timer!.isValid && tray.popover.contentViewController != nil, "No SwiftUI host/timer")
        require(tray.model.snapshot == .unavailable, "Missing initial unavailable state")

        let initial = sample()
        publishFromWorker(initial)
        tray.refresh()
        require(tray.model.snapshot.cpu == "142.5%" && tray.model.snapshot.app_count == 2, "Metrics missing")
        require(tray.model.displayedApps.first?.title == "时钟", "Unicode title lost")
        require(tray.model.adjacentApp(to: nil, forward: true) == "test.clock", "Keyboard entry skipped first row")
        require(tray.model.adjacentApp(to: "test.clock", forward: true) == "test.settings", "Down did not advance")
        require(tray.model.adjacentApp(to: "test.settings", forward: false) == "test.clock", "Up did not go back")

        for action in [TrayAction.focusApp("test.clock"), .openCenter, .quit] { tray.select(action) }
        let raw = glidepodTrayDrain(handle)!
        let actions = try JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)) as! [[String: String]]
        free(raw)
        require(actions == [["kind": "focus_app", "package_id": "test.clock"],
                            ["kind": "open_center"], ["kind": "quit"]], "Action routing failed")
        let empty = glidepodTrayDrain(handle)!
        require(String(cString: empty) == "[]", "Actions consumed twice")
        free(empty)

        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.1))
        tray.show()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.1))
        require(tray.popover.isShown, "Native popup did not open")
        let clock = tray.model.displayedApps[0]
        publish(sample(cpu: "75.0%", apps: false))
        let deadline = Date(timeIntervalSinceNow: 2.5)
        while tray.model.snapshot.cpu != "75.0%", deadline.timeIntervalSinceNow > 0 {
            RunLoop.main.run(mode: .eventTracking, before: Date(timeIntervalSinceNow: 0.05))
        }
        require(tray.model.snapshot.cpu == "75.0%", "Metrics froze in tracking mode")
        require(tray.model.displayedApps.first == clock && !tray.model.isAvailable(clock), "Open targets shifted or remained enabled")
        require(tray.model.adjacentApp(to: clock.id, forward: true) == nil, "Keyboard selected a stale row")
        tray.select(.focusApp(clock.id))
        require(tray.drain() == "[]", "Obsolete app action was queued")

        TrayMailbox.shared.publish(initial, now: ProcessInfo.processInfo.systemUptime - 6)
        publish(Data("{}".utf8))
        tray.refresh()
        require(tray.model.snapshot == .unavailable, "Stale/malformed data left measurements live")
        tray.select(.focusApp(clock.id))
        require(tray.drain() == "[]", "Stale action was queued")
        tray.popover.close()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.3))
        publish(sample(state: "Stopped", cpu: "—", apps: false))
        tray.refresh()
        require(tray.model.displayedApps.isEmpty && tray.model.snapshot.state == "Stopped",
                "Stopped state mismatch: \(tray.model.snapshot), displayed: \(tray.model.displayedApps), shown: \(tray.popover.isShown)")

        for _ in 0..<40 { tray.select(.openCenter) }
        let bounded = try JSONSerialization.jsonObject(with: Data(tray.drain()!.utf8)) as! [[String: String]]
        require(bounded.count == 32, "Action queue is not bounded")

        if CommandLine.arguments.count > 2 {
            var fixture = try JSONSerialization.jsonObject(with: initial) as! [String: Any]
            if let icons = ProcessInfo.processInfo.environment["GLIDEPOD_TRAY_FIXTURE_ICONS"] {
                fixture["apps"] = [
                    ["title": "时钟", "package_id": "test.clock", "icon_path": "\(icons)/com.android.deskclock.png"],
                    ["title": "Settings", "package_id": "test.settings", "icon_path": "\(icons)/com.android.settings.png"],
                ]
            }
            publish(try JSONSerialization.data(withJSONObject: fixture))
            tray.refresh()
            try render(tray: tray, directory: URL(fileURLWithPath: CommandLine.arguments[2]))
        }
        let timer = tray.timer!
        glidepodTrayShow(handle)
        glidepodTrayFree(handle)
        require(!timer.isValid && tray.item == nil, "Native teardown leaked timer/status item")
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.1))
        require(!tray.popover.isShown, "Deferred show reopened a disposed tray")
        print("Passed: SwiftUI popup, branded Retina/background icon, bounded app icons, per-publisher expiry, keyboard selection, background publication, Unicode focus/open/quit, single drain, tracking refresh, disabled targets, malformed/stale/stopped states, bounded queue and teardown.")
    }

    static func checkFreshness() {
        let mailbox = TrayMailbox()
        mailbox.publish(sample(validFor: 15), now: 100)
        require(mailbox.current(now: 106).state == "Running", "Background sample expired between polls")
        require(mailbox.current(now: 116) == .unavailable, "Dead background publisher stayed live")
        mailbox.publish(sample(), now: 200)
        require(mailbox.current(now: 206) == .unavailable, "Desktop freshness changed")
        mailbox.publish(sample(validFor: 1000), now: 206)
        require(mailbox.current(now: 206) == .unavailable, "Unbounded validity accepted")
        let duplicate = Data(#"{"state":"Running","detail":"","cpu":"0%","memory":"—","allocation":"—","app_count":2,"apps":[{"package_id":"a","title":"A"},{"package_id":"a","title":"B"}]}"#.utf8)
        mailbox.publish(duplicate, now: 206)
        require(mailbox.current(now: 206) == .unavailable, "Duplicate IDs refreshed stale data")
    }

    static func checkAppIcons(_ png: Data) throws {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: file) }
        try png.write(to: file)
        require(readTrayAppIcon(file.path) == png, "Cached app artwork changed")
        var snapshot = try JSONSerialization.jsonObject(with: sample()) as! [String: Any]
        snapshot["apps"] = [["package_id": "test.icon", "title": "Icon", "icon_path": file.path]]
        snapshot["app_count"] = 1
        let data = try JSONSerialization.data(withJSONObject: snapshot)
        let mailbox = TrayMailbox()
        publishFromWorker(data, mailbox: mailbox)
        require(mailbox.current().apps.first?.iconData == png, "App icon did not cross the snapshot boundary")
        require(readTrayAppIcon(nil) == nil && readTrayAppIcon("relative.png") == nil, "Missing icon fallback failed")
        try Data(repeating: 0, count: 128 * 1024 + 1).write(to: file)
        require(readTrayAppIcon(file.path) == nil, "Oversized icon accepted")
        var dimensions = png
        dimensions.replaceSubrange(16..<20, with: [0, 0, 16, 0])
        try dimensions.write(to: file)
        require(readTrayAppIcon(file.path) == nil, "Oversized image dimensions accepted")
        try Data("not a PNG".utf8).write(to: file)
        require(readTrayAppIcon(file.path) == nil, "Invalid image accepted")
    }

    @MainActor
    static func checkBackgroundIcon(_ icon: Data) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let desktop = root.appendingPathComponent("GlidePod.app")
        let agent = desktop.appendingPathComponent("Contents/Library/LoginItems/GlidePod Background.app")
        for (url, identifier) in [(desktop, "dev.glidepod.desktop"), (agent, "dev.glidepod.background")] {
            try FileManager.default.createDirectory(at: url.appendingPathComponent("Contents/Resources"), withIntermediateDirectories: true)
            let info = try PropertyListSerialization.data(fromPropertyList: [
                "CFBundleIdentifier": identifier, "CFBundlePackageType": "APPL", "CFBundleIconName": "GlidePod",
            ], format: .xml, options: 0)
            try info.write(to: url.appendingPathComponent("Contents/Info.plist"))
        }
        try icon.write(to: desktop.appendingPathComponent("Contents/Resources/GlidePod.png"))
        require(trayIcon(fallback: Data(), bundle: Bundle(url: agent)!) != nil,
                "Background tray did not reuse the enclosing app icon")
    }

    @MainActor
    static func render(tray: GlidePodTray, directory: URL) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 336, height: 360),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        let original = tray.model.snapshot
        let appearances: [(String, NSAppearance.Name)] = [
            ("light", .aqua), ("dark", .darkAqua), ("long-light", .aqua), ("long-dark", .darkAqua),
        ]
        for (name, appearance) in appearances {
            var snapshot = original
            if name.hasPrefix("long") {
                snapshot.apps += (0..<7).map {
                    TrayApp(package_id: "test.long.\($0)",
                            title: "一个很长的应用名称 · A deliberately long application name \($0)", icon_path: nil)
                }
                snapshot = TraySnapshot(state: snapshot.state, detail: snapshot.detail,
                                        cpu: snapshot.cpu, memory: snapshot.memory, allocation: snapshot.allocation,
                                        app_count: snapshot.apps.count, apps: snapshot.apps, valid_for_seconds: 5)
            }
            tray.model.refresh(snapshot, preservingApps: false)
            // A fresh host outside NSPopover avoids caching vibrancy masks as
            // colors; use an opaque native window background for these exports.
            let panel = TrayPanel(model: tray.model, icon: tray.item!.button!.image!, select: { _ in }, dismiss: {})
                .background(Color(nsColor: .windowBackgroundColor))
                .environment(\.colorScheme, appearance == .darkAqua ? .dark : .light)
            let host = NSHostingController(rootView: panel)
            window.contentViewController = host
            window.appearance = NSAppearance(named: appearance)
            window.setContentSize(host.view.fittingSize)
            window.orderFront(nil)
            RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.2))
            host.view.layoutSubtreeIfNeeded()
            let view = window.contentView!
            require(abs(view.bounds.width - 320) < 1, "Panel width grew unexpectedly")
            require(view.bounds.height < 430, "Long app list expanded instead of scrolling")
            let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds)!
            view.cacheDisplay(in: view.bounds, to: bitmap)
            try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("tray-\(name).png"))
        }
        window.orderOut(nil)
        window.contentViewController = nil
    }
}
