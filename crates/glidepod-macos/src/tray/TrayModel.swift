import Combine
import Foundation

struct TrayApp: Decodable, Equatable, Identifiable, Sendable {
    let package_id: String
    let title: String
    let icon_path: String?
    var iconData: Data?
    var id: String { package_id }

    enum CodingKeys: String, CodingKey { case package_id, title, icon_path }
}

struct TraySnapshot: Decodable, Equatable, Sendable {
    let state: String
    let detail: String
    let cpu: String
    let memory: String
    let allocation: String
    let app_count: Int?
    var apps: [TrayApp]
    let valid_for_seconds: Double?

    static let unavailable = TraySnapshot(
        state: "Status unavailable", detail: "Waiting for a current VM status sample.",
        cpu: "—", memory: "—", allocation: "—", app_count: nil, apps: [], valid_for_seconds: nil)
}

// Publication is the only entry point used off the main thread. The lock protects
// immutable values; no AppKit or SwiftUI object crosses this boundary.
final class TrayMailbox: @unchecked Sendable {
    static let shared = TrayMailbox()
    private let lock = NSLock()
    private var latest: TraySnapshot = .unavailable
    private var received: TimeInterval = -.infinity
    private var icons: [String: Data] = [:]

    func publish(_ data: Data, now: TimeInterval = ProcessInfo.processInfo.systemUptime) {
        guard var snapshot = try? JSONDecoder().decode(TraySnapshot.self, from: data),
              Set(snapshot.apps.map(\.id)).count == snapshot.apps.count,
              (1...15).contains(snapshot.valid_for_seconds ?? 5) else { return }
        lock.lock()
        let cached = icons
        lock.unlock()
        // Rust calls publication from its monitor thread. Keep icon I/O here,
        // outside both AppKit and the lock used by the main-thread reader.
        var nextIcons: [String: Data] = [:]
        for index in snapshot.apps.indices {
            guard let path = snapshot.apps[index].icon_path else { continue }
            if let icon = cached[path] ?? readTrayAppIcon(path) {
                nextIcons[path] = icon
                snapshot.apps[index].iconData = icon
            }
        }
        lock.lock()
        defer { lock.unlock() }
        latest = snapshot
        icons = nextIcons
        received = now
    }

    func current(now: TimeInterval = ProcessInfo.processInfo.systemUptime) -> TraySnapshot {
        lock.lock()
        defer { lock.unlock() }
        return now - received <= (latest.valid_for_seconds ?? 5) ? latest : .unavailable
    }
}

enum TrayAction: Equatable {
    case openCenter
    case focusApp(String)
    case quit

    var payload: [String: String] {
        switch self {
        case .openCenter: ["kind": "open_center"]
        case .focusApp(let package): ["kind": "focus_app", "package_id": package]
        case .quit: ["kind": "quit"]
        }
    }
}

@MainActor
final class TrayModel: ObservableObject {
    @Published private(set) var snapshot: TraySnapshot = .unavailable
    @Published private(set) var displayedApps: [TrayApp] = []

    func refresh(_ next: TraySnapshot, preservingApps: Bool) {
        if snapshot != next { snapshot = next }
        // Do not move a target under the pointer or keyboard focus while open.
        // Removed/stale targets remain in place but are disabled until reopening.
        if !preservingApps && displayedApps != next.apps { displayedApps = next.apps }
    }

    func isAvailable(_ app: TrayApp) -> Bool {
        snapshot.apps.contains { $0.id == app.id }
    }

    func adjacentApp(to current: String?, forward: Bool) -> String? {
        let ids = displayedApps.filter(isAvailable).map(\.id)
        guard !ids.isEmpty else { return nil }
        guard let current, let index = ids.firstIndex(of: current) else {
            return forward ? ids.first : ids.last
        }
        return ids[min(max(index + (forward ? 1 : -1), 0), ids.count - 1)]
    }
}
