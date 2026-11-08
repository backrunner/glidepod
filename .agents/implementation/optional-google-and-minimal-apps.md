# Optional Google services and minimal built-in apps

Date: 2026-11-08. Scope: the pinned Android 17 ARM64 Developer image. This extends the [Google services workflow](15-google-services-product-and-system-notifications.md) and [background runtime](adaptive-background-runtime.md).

## Behavior

Settings now distinguish Google services that are **not installed**, **on**, and **off with data preserved**. Users can turn an installed copy off, turn it on again without downloading it, or remove it and install a fresh copy later. Removal retains its existing confirmation because it clears Google account and app data. Turning services off stops their processes and Google push delivery. Other apps remain installed. Existing installation records migrate to the enabled state.

Enable and disable use the same private candidate, reboot verification, cancellation and recoverable directory promotion as installation and removal. Maintenance explicitly pauses the background VM owner. Only a verified candidate replaces the original Android instance. Calling `google-services install` on a disabled managed installation means enable. Play Store disappears from the cached app library when services are disabled or removed; Finder entries are reconciled from the next live Android catalog.

Smart start/stop now checks whether Google Play services is enabled, rather than merely installed. Enabled GMS still keeps Android online for push. Disabled GMS alone no longer prevents idle shutdown. Other existing background requirements, including active playback, downloads and configured background apps, remain independent.

The default **Minimal built-in apps** setting disables these 18 optional system packages at Android startup:

| Purpose | Packages |
| --- | --- |
| Clock, contacts UI, dialer, SMS UI | `com.android.deskclock`, `com.android.contacts`, `com.android.dialer`, `com.android.messaging` |
| Calendar, music, search UI | `com.android.calendar`, `com.android.music`, `com.android.quicksearchbox` |
| Easter egg and screensavers | `com.android.egg`, `com.android.dreams.basic`, `com.android.dreams.phototable` |
| Optional diagnostics and system-image demos | `com.android.traceur`, `com.android.devicediagnostics`, `com.android.dynsystem`, `com.android.threadnetwork.demoapp` |
| WebView demo and terminal UI | `org.chromium.webview_shell`, `com.android.virtualization.terminal` |
| SIM application UI | `com.android.stk`, `com.android.simappdialog` |

Only exact package IDs with the Android system-app flag are changed. Missing packages are skipped. A root-owned AtomicFile journal in Android records each previous enabled state before mutation. Turning the setting off restores those states on the next start, including packages that were already disabled. User app data and APKs are retained. The policy is idempotent across starts and interrupted changes. Maintenance candidates do not independently apply the policy.

Core telephony (`com.android.phone`), contacts/calendar storage providers, contact/photo/document pickers, downloads, permission controller, System UI, WebView runtime and camera remain available. The contacts UI and the contacts storage service are separate packages; disabling the former is not permission to remove the latter. GMS selection is independent of this optional-app list.

## Persistence correction found in the real VM

The initial plain Package Manager enable/disable commands changed in-memory state, but an immediate maintenance reboot could precede the delayed package-restriction write. The candidate then failed verification and rolled back. Restoring optional apps had the same ordering risk if its restoration journal was cleared first.

Both helpers now use `PackageManager.setApplicationEnabledSetting` with [`PackageManager.SYNCHRONOUS`](https://developer.android.com/reference/android/content/pm/PackageManager#SYNCHRONOUS). Android serializes the package restrictions before the helper returns. Restoration only clears its journal entry after that call succeeds. Off/on and restore states are checked again after an actual VM reboot.

## Verification

The disposable store is `.cache/adaptive-memory-test`; the original user VM disks and runtime registration were not replaced. Local evidence is under `evidence/optional-system-apps/` and is not distributed with the source.

- Rust workspace: 98 tests passed; 13 opt-in tests skipped by the ordinary suite. Android Java/DEX build, locked Clippy with warnings denied, formatting and diff checks passed.
- Real VM: all 18 optional packages disabled; restoring preserved the prior enabled/disabled states. Clock private-data sentinel survived trimming, restoration, GMS removal and reinstallation.
- GMS off: all three primary packages remained installed and disabled after reboot, no GMS/Play Store processes remained, and the background probe reported no GMS hold. A private GMS data sentinel survived disable/enable.
- Cancellation during enable restored the original disabled installation. Install on an existing disabled installation re-enabled it. Remove from the disabled state removed the three primary packages; clean reinstall returned enabled packages and retained minimal-app defaults. Enable/install validation also opened a real Play Store task.
- The real notification/storage/permission integration and independent-window/input integration passed with the minimal policy and reinstalled GMS.
- File regression passed document selection, persistent URI reads after app restart, cancellation, multi-selection, deferred saving, MediaStore publication and export idempotency. Its full test stopped at the public HTTPS DownloadManager request to `example.com`, which exceeded the fixture's 20-second deadline. The first attempt also exposed a test setup conflict with the supervisor exporter; the isolated retry removed that conflict. A subsequent control restored all optional apps and reproduced the pending request after 25 seconds: DownloadManager status 190, JobScheduler `Unsatisfied constraints: CONNECTIVITY`, with the download service enabled and no quota, battery or background restriction. This establishes that the trim list is not the cause of that failure. Full network-download acceptance is not marked passed; see `download-full-app-states.json`, `download-full-result.log` and `download-full-jobs.log`.
- Pending fixture downloads correctly kept the background probe busy even with GMS disabled. The lifecycle harness clears only that disposable fixture's download queue before requiring an otherwise idle VM.
- The separate development bundle `.cache/adaptive-ui/GlidePod Memory Test.app` contains the current UI, CLI and Android helpers; deep ad-hoc signature verification passed. The app library rendered, but native Settings navigation did not respond to automated clicks, so the Settings button interaction is not claimed as passed.

The repeatable lifecycle test creates its own non-sensitive data sentinels, captures original app states, exercises trim/restore/off/cancel/on/remove/reinstall, and leaves Android stopped. It requires a disposable, already registered and GMS-equipped pinned image plus the verified local installer archive:

```sh
rtk proxy python3 tools/build-android.py
rtk proxy cargo build --workspace --locked
rtk proxy python3 tools/check-optional-apps.py --data-dir .cache/adaptive-memory-test
```

CLI controls:

```sh
rtk proxy target/debug/glidepodctl --data-dir .cache/adaptive-memory-test google-services disable
rtk proxy target/debug/glidepodctl --data-dir .cache/adaptive-memory-test google-services enable
rtk proxy target/debug/glidepodctl --data-dir .cache/adaptive-memory-test configure --builtin-apps minimal
rtk proxy target/debug/glidepodctl --data-dir .cache/adaptive-memory-test configure --builtin-apps full
```

## Limits and next image-build work

This change disables optional packages reversibly. It does not rebuild or shrink the base image, delete its APKs, qualify the rootless Personal image, or establish a fixed RAM saving. Already dormant apps may have little resident-memory cost. GMS off removes its live processes; enabled GMS and Android's essential services still require memory. Host memory must use the [backing-aware measurements](resident-memory-audit.md), not a sum of aliased VM regions.

No Google account was signed in during these tests. Private-data preservation and package/task checks do not establish authenticated-session continuity, device certification or end-to-end remote FCM delivery. Those remain separate acceptance checks.

A future source-built desktop image can omit the same allowlisted UI/demo packages and their artifacts from its product manifest. That needs a separate reproducible image build and boot/intent/provider/notification regression pass; broad name-based APK deletion or removal of service providers is not part of this implementation.
