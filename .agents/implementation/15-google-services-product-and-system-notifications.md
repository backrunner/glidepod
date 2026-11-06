# Optional Google services and Android system notification delivery

Date: 2026-11-05. This supersedes the engineering-only installer status in [the initial investigation](14-google-services-and-notifications.md). The requested notification scope is the Android notification center, including Android framework, preinstalled and third-party applications, persistent notifications and group summaries.

## Delivered product workflow

Settings → Google services now installs the pinned Android 17 ARM64 core package, displays progress, allows cancellation, offers removal and exposes recovery when an interrupted transaction remains. Open Play Store starts Android when necessary. Play Store is also visible in the app library despite being a system application. This currently operates on the supported Developer runtime; the rootless Personal runtime is not implemented by the underlying adapter.

The native installer downloads the 519 MiB community archive on demand through an ephemeral `NSURLSession`, so host system proxy configuration applies to the download. No Python, Cargo, recovery ZIP script or separately installed command-line tool is needed by the app. Downloaded Google binaries are not included in the application bundle or repository. The package size, SHA-256, SDK, ABI, complete core file manifest, file types, path uniqueness and expanded-size limits are checked before installation. ZIP members are streamed to disk and individual files are streamed to Android; the entire archive is never loaded into memory. A successful transaction removes its download/extraction cache. A failed operation retains only a fixed-name verified archive for retry and removes extracted payloads.

The 246 MiB core payload includes the GMS APEX, GSF, Play Store and required partner/configuration components; optional Google search, speech and wellbeing apps are excluded. The 4 vCPU/4 GiB guest configuration is retained. Resident GMS memory is workload-dependent and has not been qualified as a fixed budget.

CLI equivalents are `glidepodctl google-services install`, `remove`, `recover` and `status`. `--data-dir` selects the store; `--json` emits machine-readable progress. SIGINT requests cancellation. `glidepodctl gms` remains the separate live inventory diagnostic and does not infer API availability, login, FCM or certification from installed packages.

## Transaction and recovery

A maintenance lock excludes profile/configuration changes and other installers. Android is stopped and its session and disk ownership are checked before cloning the existing image files into a private candidate store. APFS clones preserve sparse disks without copying their full logical size. A non-cloning filesystem must have sufficient free space for the full copy. The original disks remain untouched during download, preparation, guest remount, installation, removal and boot verification.

The candidate is started by the same owned supervisor as ordinary Android sessions. It has its own lifetime pipe and session lock. Permission allowlists precede privileged APK/APEX files. Guest storage and overlay space are checked independently. Each transferred file is verified by a guest SHA-256. Installation requires all three enabled system packages and a real Play Store task; removal requires their absence after reboot. The installation record binds the package and complete file manifest to the Android image inventory. Changing the registered image blocks boot while managed GMS remains, with an actionable instruction to restore the compatible runtime.

A durable, directory-synced journal brackets promotion:

1. `preparing`: only the private candidate can change.
2. `promoting`: preserve the original instance under a backup name, then move the verified stopped candidate into place; sync both parent directories.
3. `committed`: keep the new instance and remove the previous instance and preparation files.

Before commit, recovery restores the original instance; after commit, recovery finishes cleanup. A new-profile installation removes only its uncommitted candidate. The normal runtime worker performs recovery before boot, and Settings also exposes an explicit recovery action. Candidate session/disk locks prevent cleanup while a surviving engine still owns its files. Unknown maintenance directories and unknown journal formats are preserved and reported rather than deleted. Cancellation is checked during download, extraction, boot polling and between guest file transfers; once the short promotion phase begins, it completes or recovers using the journal.

Removal operates on a new copy of the *current* instance, preserving app data created after GMS installation. It clears managed Google component data, removes their system updates and known APEX updates, then removes only the pinned manifest's system files. It never restores an old pre-install data snapshot as an uninstall shortcut. The user-facing removal confirmation explains that Google account/component data is removed.

## Android notification center coverage

The listener no longer filters by application system flag, Android user ID, ongoing flag or group-summary flag. Every entry exposed by Android's `NotificationListenerService` is represented, including the framework package `android` and notifications addressed to all users. Android's own profile/access restrictions still apply; the host does not bypass them. Secret entries remain visible as generic private notifications; other private entries use their public version or generic content. Text, inbox lines, labels, group identity, clearability and click availability are bounded. The relay keeps no separate persistent content archive; macOS controls retention of its delivered notifications.

Snapshots are read in stable-key pages of 64 with a revision consistency check, rather than dropping entries beyond the former 256-item limit. At most 4,096 active entries are accepted; exceeding that bound or a changing snapshot reports an explicit synchronization error instead of silently presenting a truncated snapshot as complete. The host retains one snapshot plus content hashes/minimal identities, not duplicate notification bodies. macOS delivery is limited to eight changes per second; unseen entries precede updates, and initial history, ongoing entries, summaries and replacements are delivered quietly. Android cancellation removes the matching native entry. A native dismissal cancels Android only when Android permits dismissal.

Clicks validate VM epoch, notification identity, package and post time, then execute the original Android `PendingIntent`. Android 17 blocks a background listener from launching arbitrary activities. Therefore an expiring, single-use token launches a DUMP-protected foreground helper on the managed display before sending the intent. No background-launch permission or attestation is spoofed. Existing app windows are reused where possible; a notification without an existing window can allocate a dedicated display, including when the originating package has no launcher activity. The actual target task owns window cleanup. Original pending intents control their own semantics; broadcast/service intents need not produce a visible activity.

The user preference survives restarts. The relay reinstalls/updates its helper and reconnects once per VM epoch when forwarding is enabled and macOS permission is granted. Android revocation within an epoch is reported rather than silently overridden. Forwarding requires the host application and Android to remain running. macOS may apply its own Focus, presentation and Notification Center retention rules; Android RemoteViews, inline reply fields and media control layouts are not rendered as native controls.

## Validation

Local evidence: `evidence/gms-product-2026-11-05/`.

- The native downloader fetched the pinned archive successfully. A real existing Android instance completed installation, reboot verification and Play Store launch. The pre-existing data sentinel survived.
- Removal completed and a subsequent real boot confirmed GMS, GSF and Play Store were absent; the same pre-existing data sentinel survived.
- SIGINT during payload installation restored the original instance and removed the transaction journal.
- SIGKILL during candidate startup left the durable journal; the next normal Android start automatically recovered. Original data and the original GMS-free package state survived.
- Unit fault cases exercise each promotion interruption point, committed cleanup, first-install recovery, repeated recovery, cancellation and maintenance exclusion. Archive tests reject traversal, shell characters, case collisions, unexpected files and SDK mismatches.
- The real management suite passes with GMS installed: Android framework persistent notifications, a preinstalled system-app notification, public Unicode content, private and secret redaction without dropping entries, persistent entries, group summaries, multi-page synchronization of more than 64 notifications, updates, dismissal, permission denial, provider isolation, original pending-intent activation and a newly allocated notification display.
- Final workspace format, Clippy, 61 ordinary Rust tests (six opt-in tests skipped in that run), build, package and signature checks pass; results are saved in `checks.json`. The development bundle is ad hoc signed; no publication or production signing is implied.

Google sign-in, remote FCM delivery, certification and Play Integrity remain separate unverified capabilities. The host download using system proxies does not implement transparent proxying of all VM traffic. FCM/proxy compatibility remains the work described in [the network plan](13-vm-network-and-proxy-plan.md). The rebuilt native app launches and a system capture records the real Apps window. The computer-use provider reports `cgWindowNotFound`; native investigation confirms the frontmost process is `loginwindow`, so the session is locked and app clicks cannot be delivered. The user explicitly deferred unlocking and native acceptance in favor of code and guest validation. Settings interactions and native banner/click/dismiss acceptance remain pending; guest checks do not establish native banner acceptance. See the [source review](16-source-review.md) for subsequent fixes and checks.

## Primary references

- [Android NotificationListenerService](https://developer.android.com/reference/android/service/notification/NotificationListenerService).
- [Android background activity launch restrictions](https://developer.android.com/guide/components/activities/background-starts).
- [Pinned MindTheGapps release](https://github.com/MindTheGapps/17.0.0-arm64/releases/tag/MindTheGapps-17.0.0-arm64-20261022_132445).
