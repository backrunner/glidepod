# Per-app macOS Dock entries

## Behavior

Installed library apps automatically receive managed bundles in `~/Applications/GlidePod Apps`, exposed through Applications and indexed by Spotlight; see [automatic app discovery](20-finder-and-spotlight-apps.md). Opening an app also creates or refreshes its entry when needed. Each open app has a small, regular AppKit process with its own bundle identifier, Android label and icon. The entry can also be opened from Finder or kept in the Dock.

Clicking the entry, selecting **Show Window**, or selecting it through the app switcher requests focus for its existing GPUI window. The standard Dock **Quit** action requests closure of that window only. Closing the native window, finishing its Android task with Back, or losing the host process ends the helper. An unexpected helper exit also closes only its associated window. macOS may retain an unpinned, stopped entry in its recent-apps section according to the user's Dock settings.

The existing package/display-scoped guest cleanup remains authoritative. Closing an entry removes its task and display; it does not uninstall the package, erase its data, force-stop background services, or stop another app. The common Android VM still belongs to the existing runtime owner.

## Ownership and transport

- `AppLauncher.swift` now runs an `NSApplication` delegate instead of forwarding a URL and exiting. It uses the public [Dock menu](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationdockmenu(_:)) and [termination](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldterminate(_:)) callbacks.
- `glidepod-macos::dock::Session` owns a private Unix socket for one window. A per-package file lock rejects duplicate owners and permits safe stale-socket cleanup. The directory is user-owned with mode 0700, sockets have mode 0600, command size and queue capacity are bounded, and reads have timeouts.
- The GPUI window drains commands independently of the management center, including while the center is closed. Focus and input ownership remain in the single existing desktop process.
- Host-started helpers carry `--glidepod-managed`; a delayed helper must exit if its window has already disappeared. Finder cold starts can route through `glidepod://launch/` and wait for the window socket. A custom `GLIDEPOD_DATA_DIR` is retained for subsequent cold starts.
- Launcher generation detects helper, label, icon, host-path and data-directory changes. It signs a complete replacement before publishing, rolls back a failed rename, and refuses to replace a bundle with a different ownership marker. Existing one-shot shortcuts migrate on use.

This is per-app Dock control with shared window hosting. The native window owner, menu bar, application-wide Hide, Mission Control grouping and the main process's Quit remain GlidePod-wide. It is not full Parallels Coherence parity or a separate VM/rendering process for each app.

## Validation on 2026-11-08

- Workspace Rust tests: 81 ordinary tests passed before the launcher migration test was added; the final focused macOS suite passed all 7 tests, including that new test. The 12 opt-in environment/guest tests were not part of that workspace run.
- Locked workspace Clippy with warnings denied, locked build, Swift compilation with warnings treated as errors, development packaging and strict ad-hoc signature checks passed. Cargo reports pre-existing future-incompatibility notices for dependency crates.
- `tools/check-dock-native.py` exercises the actual AppKit delegate against a Unix socket: menu target/action, reopen/activation focus requests, Quit, fragmented readiness, host disconnect and a host-started helper arriving after the window closes. It invokes callbacks in process and does not synthesize desktop input.
- Rust socket tests cover two independently connected app sessions, duplicate ownership, split commands, window drop and helper disconnect. The launcher test verifies migration, code signing and preservation of an unrelated bundle.
- In the live disposable Android session, Clock and Settings opened in GPUI windows. `NSWorkspace` reported two distinct regular applications named Clock and Settings, each with an icon. Clicking Android Back in Clock closed its window and helper while Settings stayed running. Terminating the Settings helper closed its window while the management center remained open.
- Terminating the test desktop process ended both helpers. Starting the Clock launcher executable without a running desktop cold-started the packaged host, reopened the real Clock window in the saved disposable data directory, and retained exactly one Clock helper. The regenerated bundles passed strict signature verification and their helper hashes matched the packaged binary; metadata evidence is in `evidence/dock-20261108/bundles.json`.
- The computer-use interface timed out when selecting Dock or the windowless helper directly. Physical Dock icon/menu clicks, visual Dock placement and app-switcher presentation are therefore not claimed as verified by this run. The native callback tests and live process/window checks are separate evidence.

The local development bundle is `dist/GlidePod.app`. No release, notarization, commit or push is part of this change.
