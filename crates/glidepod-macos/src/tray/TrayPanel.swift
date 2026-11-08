import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct TrayPanel: View {
    @ObservedObject var model: TrayModel
    let icon: NSImage
    let select: (TrayAction) -> Void
    let dismiss: () -> Void
    @FocusState private var focusedApp: String?

    var body: some View {
        VStack(spacing: 12) {
            HStack(spacing: 9) {
                Image(nsImage: icon).resizable().frame(width: 32, height: 32)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text("GlidePod").font(.headline)
                    Text("Android · \(model.snapshot.state)")
                        .font(.subheadline).foregroundStyle(.secondary)
                        .help(model.snapshot.detail)
                }
                Spacer(minLength: 0)
            }

            Grid(horizontalSpacing: 16, verticalSpacing: 5) {
                metric("CPU", value: model.snapshot.cpu,
                       help: "VM process CPU. 100% means one fully occupied host CPU core.")
                metric("Memory", value: model.snapshot.memory,
                       help: "Physical memory footprint of the Android VM process on this Mac.")
                metric("Assigned", value: model.snapshot.allocation,
                       help: "Resources assigned to Android. This is not measured memory usage.")
            }
            .font(.subheadline)

            Divider()
            VStack(spacing: 4) {
                HStack {
                    Text("Open Apps")
                    Spacer()
                    Text(model.snapshot.app_count.map(String.init) ?? "—").monospacedDigit()
                }
                .font(.subheadline).foregroundStyle(.secondary)
                .padding(.bottom, 2)
                if model.displayedApps.isEmpty {
                    Text(model.snapshot.app_count == nil ? "App status unavailable" : "No open apps")
                        .font(.body).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(.vertical, 6)
                } else {
                    ScrollViewReader { scroll in
                        ScrollView {
                            VStack(spacing: 0) {
                                ForEach(model.displayedApps) { app in
                                    TrayAppRow(app: app, available: model.isAvailable(app),
                                               focused: focusedApp == app.id) {
                                        select(.focusApp(app.id))
                                    }
                                    .focused($focusedApp, equals: app.id)
                                    .onKeyPress(.return) {
                                        guard model.isAvailable(app) else { return .ignored }
                                        select(.focusApp(app.id))
                                        return .handled
                                    }
                                    .id(app.id)
                                }
                            }
                        }
                        .frame(height: CGFloat(min(model.displayedApps.count, 6)) * 30)
                        .onMoveCommand { direction in
                            guard direction == .up || direction == .down else { return }
                            focusedApp = model.adjacentApp(to: focusedApp, forward: direction == .down)
                        }
                        .onChange(of: focusedApp) { _, id in
                            if let id { scroll.scrollTo(id) }
                        }
                    }
                    .padding(.horizontal, -6)
                }
            }
            Divider()
            HStack {
                Button("Open GlidePod") { select(.openCenter) }
                    .keyboardShortcut("o", modifiers: .command)
                Spacer()
                Button("Quit") { select(.quit) }
                    .keyboardShortcut("q", modifiers: .command)
                    .help("Quit GlidePod").accessibilityLabel("Quit GlidePod")
            }
            .controlSize(.small)
        }
        .padding(14)
        .frame(width: 320)
        .fixedSize(horizontal: false, vertical: true)
        .onExitCommand(perform: dismiss)
    }

    private func metric(_ title: String, value: String, help: String) -> some View {
        GridRow {
            Text(title).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(value).monospacedDigit().lineLimit(1)
                .gridColumnAlignment(.trailing)
        }
        .help(help).accessibilityElement(children: .combine)
    }
}

private struct TrayAppRow: View {
    let app: TrayApp
    let available: Bool
    let focused: Bool
    let action: () -> Void
    @State private var hovered = false

    private var highlighted: Bool { available && (hovered || focused) }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(nsImage: app.iconData.flatMap(NSImage.init(data:))
                      ?? NSWorkspace.shared.icon(for: .applicationBundle))
                    .resizable().interpolation(.high).frame(width: 20, height: 20)
                    .accessibilityHidden(true)
                Text(app.title).lineLimit(1).truncationMode(.tail)
                Spacer(minLength: 0)
            }
            .font(.body)
            .foregroundStyle(highlighted ? Color(nsColor: .selectedMenuItemTextColor) : .primary)
            .padding(.horizontal, 6).frame(height: 30)
            .contentShape(Rectangle())
            .background(highlighted ? Color(nsColor: .selectedContentBackgroundColor) : .clear,
                        in: RoundedRectangle(cornerRadius: 5))
            .opacity(available ? 1 : 0.4)
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .disabled(!available)
        .onHover { hovered = $0 }
        .help(available ? "Show \(app.title)" : "\(app.title) is no longer open")
        .accessibilityLabel(app.title)
        .accessibilityHint("Show the existing app window")
    }
}

// Cached guest icons are bounded PNG files. Read them off AppKit's main thread;
// a missing/invalid icon must never stop an app row from opening its window.
func readTrayAppIcon(_ path: String?) -> Data? {
    guard let path, path.hasPrefix("/"),
          let file = try? FileHandle(forReadingFrom: URL(fileURLWithPath: path)) else { return nil }
    defer { try? file.close() }
    guard let data = try? file.read(upToCount: 128 * 1024 + 1),
          data.count <= 128 * 1024, data.count >= 24,
          data.prefix(8) == Data([137, 80, 78, 71, 13, 10, 26, 10]) else { return nil }
    let width = data[16..<20].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    let height = data[20..<24].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    guard (1...1024).contains(width), (1...1024).contains(height) else { return nil }
    return data
}
