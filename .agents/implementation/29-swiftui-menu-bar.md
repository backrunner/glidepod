# SwiftUI menu bar panel

Implemented and reviewed on 2026-11-08. This replaces the original Objective-C text menu with a 320-point native SwiftUI panel, presented by `NSPopover` from an AppKit status item.

## Presentation and ownership

- The header shows the existing GlidePod app icon and Android state. CPU, physical memory footprint and assigned resources use three aligned text rows, with semantic system fonts and colors. There are no metric cards, decorative symbols or custom glass layers.
- Open Android windows appear as 30-point action rows keyed by package ID, with their cached app artwork. Missing icons use the system application icon. Six rows are visible before scrolling. Long labels truncate without expanding the panel; the full name remains accessible. The previous checkbox-like outline and external-link arrows are removed.
- Open GlidePod and Quit retain their Rust-owned behavior. Open GlidePod has no ellipsis because it requires no further input. Escape dismisses the panel. Command-O and Command-Q are available within the panel. App rows use standard button activation, semantic selection colors and keyboard focus, with up/down traversal that skips unavailable rows and scrolls the selected item into view.
- The status item uses a 20-point, non-template color image from the same `Assets.car`/complete ICNS resources as the application. The embedded background agent resolves its enclosing desktop bundle. Direct Cargo runs use the existing exported app artwork embedded by Rust. No replacement system symbol or separately drawn logo is used.
- The desktop and existing background agent both call the same C ABI. There is no additional process, VM controller or IPC service.

## Integration and failure handling

`crates/glidepod-macos/src/tray/` contains the Swift model, panel and AppKit controller. Cargo compiles these with the selected Xcode Swift compiler in Swift 6 mode, with warnings denied and the configured macOS deployment target. The static archive links into the existing Rust executables; runtime libraries resolve to system Swift libraries.

Rust still owns monitoring, VM state and action execution. A bounded JSON snapshot crosses the C ABI into a locked, immutable Swift mailbox. AppKit and SwiftUI stay on the main actor. Common-mode timer updates continue while the panel is open. Desktop snapshots expire after five seconds. Background snapshots, sampled every five seconds, expire after fifteen seconds; this fixes a review finding where a healthy background sample could expire between polls. Invalid expiry values, malformed data and duplicate IDs do not refresh the freshness timestamp.

The window-open path carries the existing guest icon-cache path into the tray snapshot. The publishing thread reads bounded PNG data outside the mailbox lock, caches successful reads while those apps remain present, and includes immutable image data in the snapshot. The UI never opens these files or waits for a SwiftUI appearance task. Files exceeding 128 KiB or 1024 pixels per side fall back to the system icon.

While open, app rows retain their positions and unavailable targets become disabled. Selection also checks the freshest mailbox contents, preventing a removed or expired target from being queued between timer ticks. The 32-action queue drains once. Disposal invalidates the timer, removes the status item and prevents a deferred show from reopening the panel.

## Validation

- `cargo build --workspace --locked` passed, including Swift compilation and Rust/Swift linking.
- `cargo test --workspace --locked` passed 110 tests; 17 environment-dependent tests were ignored. Workspace Clippy passed with warnings denied. Existing upstream future-compatibility notices remain for `block` and `proc-macro-error2`.
- `python3 tools/check-tray-native.py --render-dir evidence/tray-review-20261108 --icons-dir .cache/host-files-20261107/developer/icons` passed native popup creation, color/Retina icon setup, enclosing-app icon lookup, worker-thread publication, Unicode app actions, open/quit payloads, single consumption, common-mode refresh, stable disabled targets, stale/malformed/stopped snapshots, queue limits and teardown. Review checks additionally cover desktop/background expiry boundaries, invalid expiry, duplicate IDs, bounded icon files/dimensions, missing artwork and keyboard selection of available rows. The callback harness disables popover animation because it pumps `RunLoop` rather than `NSApplication.run`.
- Actual SwiftUI view exports in light and dark appearances were visually inspected. These use fixture values on an opaque native background; they are not live VM measurements or screenshots of the menu bar.
- The isolated native preview exposed accessible single-action app rows and a scroll area for nine fixtures. Clicking Clock produced the expected package-scoped action payload. Later desktop capture attempts returned `cgWindowNotFound`, so physical keyboard navigation and final desktop captures are not counted as passed. The preview bundles its fixture icons instead of reading external test directories; process sampling confirmed its main run loop remained responsive after removing a synchronous fixture-file read.
- The packaged application was opened through computer use. **GlidePod → VM Status** displayed the actual popover with the app icon and stopped/empty state. **Open GlidePod…** returned to the management center. **Quit** was clicked in the popover and the application inventory confirmed the desktop had exited.
- The development bundle was rebuilt with `tools/package-dev.py --skip-build` and its strict ad-hoc signature verified.

The global status-bar click, physical VoiceOver/keyboard navigation, live Android-window selection, and runtime compatibility on macOS 15 were not separately verified in this change. The VM monitoring and focus behavior have unit/callback coverage; this record does not replace live-guest acceptance. Packaging is a local ad-hoc development build, not a notarized public release.

## Design references

The review follows Apple's current [menu guidance](https://developer.apple.com/design/human-interface-guidelines/menus) on meaningful icons, unavailable commands and ellipses, [popover guidance](https://developer.apple.com/design/human-interface-guidelines/popovers) on compact sizing and automatic dismissal, and [Liquid Glass guidance](https://developer.apple.com/videos/play/wwdc2025/219/) on avoiding stacked glass and unnecessary visual emphasis. System presentation supplies the macOS material; custom glass effects are not applied to individual content rows.
