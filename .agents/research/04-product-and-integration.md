# Product behavior and macOS integration

## Interface model

The center is an application library with a quiet sidebar: Apps, Running, Android, Graphics, Widgets and Settings. Installation and VM status are visible without opening an advanced page. Developer/root settings are grouped with the active profile. The prototype illustrates a light native-style interface and a Mac desktop scene, with a dark appearance toggle.

| Surface | Primary action | Secondary behavior |
| --- | --- | --- |
| Apps | Open an app / Install APK | Search, inspect, show Mac entry, uninstall |
| Running | Focus a window | Close task or explicitly force-stop app |
| Android | Start / Stop / Restart | Resources, profiles, backup/recovery, root |
| Graphics | Select Automatic / Metal / Software | Render scale, frame cap, diagnostics, per-app overrides |
| Widgets | Add a live widget | Configure, pin/unpin, stale/offline status |
| Settings | Integration preferences | File association, clipboard, notifications, launch entries, theme |
| App window | Interact with Android | Back, rotate, game input capture, native full-screen |
| Menu bar | Runtime state and recent apps | Start/stop, center, permissions and quit |

Use descriptive errors with an action: “This APK only contains x86 libraries. Choose an ARM64 build.” Keep implementation details such as graphics transport names in diagnostics, not ordinary installation copy. All prototype content is English; production should use localization resources from the start.

## First run and APK installation

First run checks OS, CPU architecture, Hypervisor availability, free space and network, then offers a verified Android runtime download and recommended resources. Display download size, expansion size, provenance, version and progress. Interrupted downloads resume only after range/hash checks; partial files are never treated as an installed image. A user can inspect the library while provisioning. External launch/install requests wait in a visible queue.

Register `.apk` using `CFBundleDocumentTypes`, a suitable declared/imported UTI, and an alternate handler rank. Receive `application:openFiles:` and drag/drop. Offer the normal macOS choice to change the default handler; do not forcibly overwrite another application's association. An APK double-click opens the same installer sheet as the toolbar and Finder drop.

Installation is a transaction:

1. The receiving UI opens the selected file and passes a descriptor to the runtime. The inspector parses ZIP and binary Android manifests with strict resource limits.
2. Show package label/ID, version, signer fingerprint, SDK requirements, native ABIs, permissions, size and target profile. A parseable manifest is not proof of safety. Do not label an unknown signer as trusted.
3. Check supported ABI, `minSdkVersion`, image compatibility, signing lineage for update, duplicate versions and disk headroom including staging. Downgrade/uninstall-with-data-loss requires a distinct action; never silently replace an incompatible signing identity.
4. Start the VM if necessary; use Android `PackageInstaller` sessions or equivalent authenticated package service. Stream with backpressure and an idempotency key. ADB is a bring-up/debug transport, not the long-term authority for the normal installer.
5. Cancellation before commit abandons the session and removes staging. Once commit starts, report completion or reconciliation, not a false cancellation. Android's package result/event confirms success.
6. Persist the package and then create the Mac entry atomically. If entry creation fails, installation remains successful with “Repair Mac entry”; do not uninstall a working app.
7. Offer Open app and Show in Applications. Uninstall explicitly identifies app, profile and data loss; remove only registry-owned shims, widgets and grants after guest confirmation.

v0.1 guarantees single APK handling. Add `.apks` split sets next, validating package, version, signer and ABI/density selection as one session. `.xapk`/`.apkm` and OBB layouts are separately versioned import formats with bounded extraction; reject incomplete sets with a precise explanation. `.aab` is a publishing artifact, not a directly installable APK.

## Application entries and identity

Each launchable activity gets an entry keyed by `(profile_id, Android user, package_id, component)`. Stable identifiers include a hash to avoid label collisions and lossy identifier conversions. A package can expose more than one launcher activity. The first scope can show the default activity and offer additional activities in the inspector.

Create registry-owned bundles under `~/Applications/GlidePod Apps/` with icon, `CFBundleDisplayName`, stable `CFBundleIdentifier` and a small universal launcher executable. Use `LSUIElement=false` for normal Dock/app-switcher identity. Preserve the signed launcher executable; generated bundle metadata requires local bundle signing and a tested trust strategy. Never ship a Developer ID private key to sign user-generated entries. Validate registry identity and code hashes at XPC connection time. Test application movement, update, label changes and repair.

Entries should participate in LaunchServices and Spotlight. Older macOS versions expose Launchpad; current Apple documentation describes an Apps view in Spotlight. Use the user-facing term “Mac app entry,” and test discoverability rather than relying on undocumented Launchpad database edits. [Apple's application discovery guidance](https://support.apple.com/guide/mac-help/open-apps-in-spotlight-mh35840/mac).

Each app process can own several task windows, so Dock, Command-Tab, Command-W and Command-Q have coherent app identity. Native notifications initially use GlidePod's signed notification identity and include the Android app name/icon. Per-app notification identity requires separate qualification; generated bundles should not be assumed to have distribution-time entitlements.

## Host integration behavior

| Feature | Design | Required edge case |
| --- | --- | --- |
| Clipboard | Opt-in text sharing; image/file support later; scoped to current app and explicit action where practical | Clipboard loops, passwords, background polling and clipboard ownership |
| Files | Selected file descriptors; import/export broker; later Android DocumentsProvider | TCC, cancellation, large files, symlink escape and expired grants |
| Open links | Host browser for permitted web URLs; validate scheme and user gesture | Intent loops, custom schemes, unsolicited background opens |
| Notifications | Mirror allowed Android notifications, revoke/dismiss mapping and action tokens | Stale actions, VM off, no host permission and sensitive content |
| Media | Core Audio output; now-playing/media commands routed to active Android session | Focus conflict with Mac media apps, sleep/wake and sample-rate changes |
| Camera / microphone | Off until requested; host permission plus per-guest-app policy | No hardware, denied TCC, visible capture indicators |
| Accessibility | Host controls have semantic labels; phased Android semantic tree bridge | Secure text, focus navigation, TalkBack conflict and stale nodes |
| Menu bar / login | Same-user background service with explicit login preference | Service disabled in System Settings, logout and stale helpers |

Apps remain virtualized: they do not get transparent access to Keychain, iCloud, Touch ID, USB, Bluetooth or the entire filesystem. Add individual bridges only with clear permissions and device-specific tests.

## Root and profiles

“Root” always means **Android guest root**, never macOS root. Personal defaults to a production-oriented image with root disabled. Developer uses a separate userdata disk and a userdebug-capable image. The internal system bridge has limited privileged APIs regardless of whether a user-facing root shell is enabled.

Offer two separate controls: **Developer ADB** (authenticated private/local endpoint) and **Root shell** (guest policy). `adb root` is not an application `su` permission manager. v0.1 may implement developer root shell first, with per-app `su` grants as a later audited module. Label this scope in the UI. A root switch changes desired policy, explains restart impact and applies only after a successful reboot/verification. Show desired and effective state if they differ.

For later per-app root, use a reviewed `su` broker with Android UID/package/signer-scoped grants, one-time/session grants, expiration, revoke, an audit trail and denial by default. Do not disable SELinux globally. Do not bundle Magisk as a casual dependency or claim its license disappears because a user can download it. Turning root off does not undo modifications made by root; offer restoration of a clean image/profile backup when needed. Enabling root does not improve Play Integrity certification.

## Two kinds of widgets

**Live Android widgets:** a platform integration app implements `AppWidgetHost`, owns persistent widget IDs, performs binding/configuration and lays out `AppWidgetHostView` in a guest display. Stream that display through the existing window pipeline into a native desktop panel. Clicks and scrolling execute in Android, including `PendingIntent` actions. Honor widget minimum/maximum sizes, configuration cancellation, provider removal and rebind after restore. [Android widget host contract](https://developer.android.com/develop/ui/views/appwidgets/host).

Use normal public AppKit window levels and Spaces behavior; do not inject views into Finder. “Pin to desktop” is a floating/desktop-style panel preference, not membership in the system widget gallery. A live widget can keep Android running only with explicit user preference. When stopped, freeze the last safe preview with a timestamp and a clear Start Android action.

**Native Mac widgets:** ship a small SwiftUI/WidgetKit extension for runtime status, favorite-app launch actions and optional cached Android-widget previews. Share a sanitized snapshot using an App Group; validate capabilities and signing for direct Developer ID distribution. The extension has its own sandbox/lifecycle requirements even though the main app is distributed outside the App Store. A snapshot opens the real widget/app when selected; it is not an arbitrary live Android surface inside WidgetKit. [WidgetKit](https://developer.apple.com/documentation/widgetkit/), [supported content](https://developer.apple.com/documentation/widgetkit/creating-a-widget-extension).

WidgetKit controls refresh timing; mark stale previews and use App Intents only for supported actions. Never claim interactive RemoteViews automatically become SwiftUI views. The native extension's packaging/entitlement experiment is a release gate; the first usable fallback is a live desktop panel, with native widgets labeled as deferred if that gate fails.

## Prototype coverage

The prototype provides library filtering, application details, a simulated installer with failure scenarios, running app windows, drag/resize, root/restart flow, profile switching, VM start/stop, graphics preferences, game input capture, widget panels, Mac app-entry browsing, appearance settings and reset. Its sample apps and game are fictional. All installation and graphics evidence is simulated; no APK parser, signing, actual widget extension or guest bridge is present.
