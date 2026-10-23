import AppKit

// A small Finder/Launchpad entry. Android windows remain owned by GlidePod.
guard let package = Bundle.main.object(forInfoDictionaryKey: "GlidePodPackage") as? String,
      let url = URL(string: "glidepod://launch/" + package) else { exit(1) }
let workspace = NSWorkspace.shared
guard let app = workspace.urlForApplication(withBundleIdentifier: "dev.glidepod.desktop") else {
    let alert = NSAlert(); alert.messageText = "GlidePod is not installed";
    alert.informativeText = "Open GlidePod once, then try this app again."; alert.runModal(); exit(1)
}
workspace.open([url], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration()) { _, error in
    if let error = error { fputs(error.localizedDescription + "\n", stderr); exit(1) }
    exit(0)
}
RunLoop.main.run()
