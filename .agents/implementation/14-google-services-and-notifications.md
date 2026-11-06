# Google services, optional installation and host notifications

Product follow-up: [optional installation and complete Android notification center coverage](15-google-services-product-and-system-notifications.md). The engineering-only status below describes the initial investigation.

Date: 2026-11-05. Scope: the pinned Android 17 ARM64 Developer runtime. The user confirmed that “CMS” meant GMS and requested investigation of a one-click installation path. **A production GMS installer, Google account login, FCM delivery and Google certification are not qualified by this work.**

## Current guest and delivered diagnostic

The existing isolated runtime reports Android 17 / SDK 37 / `arm64-v8a`, `userdebug` and `test-keys`. A live Package Manager query finds none of `com.google.android.gms`, `com.google.android.gsf` or `com.android.vending`. The unmodified AOSP image therefore cannot currently provide Google Play services or the Play Store.

`glidepodctl --data-dir PATH gms` now reports these three prerequisites, their enabled/system state, versions and observed SHA-256 signing-certificate digests. It distinguishes `missing_components`, `disabled_components` and `present_unverified`. It never turns package presence into API health, authentic Google provenance, successful account login, working FCM, Play Protect certification or Play Integrity acceptance. Those fields remain explicitly `not_tested`. The helper does not read accounts, Android/GSF IDs, advertising IDs, messaging tokens or credentials. The standard owned bridge JAR is cached in the guest to run the query.

Google's supported developer test configurations include compatible devices with the Play Store and Android Emulator Google APIs images. An Android Emulator image is not a drop-in Cuttlefish disk image. A compatible image and an available SDK service are separate prerequisites; the official API probe must ultimately call `isGooglePlayServicesAvailable()` from an original Android test application.

## A concrete optional-install route

MindTheGapps has an Android 17 ARM64 release matching this guest. The candidate is pinned in `engine/google-apps-lock.json`:

- Release: `MindTheGapps-17.0.0-arm64-20261022_132445`.
- Archive: 544,284,463 bytes; SHA-256 `a6844eeb2613541558237e467b20feb01f8b45b2ebab0109834140ea59b6ca4d`, verified against the GitHub release asset digest.
- Package `build.prop`: `arch=arm64`, `version=37`, `version_nice=17.0.0`.
- Default core payload: 258,085,072 bytes (about 246 MiB), retaining GMS, GSF, Play Store, partner setup, feedback and system configuration. Optional search, speech, accessibility and wellbeing applications are not added by this core variant. `--variant full` selects the broader 716,481,828-byte payload. Android 17's Play services payload is an **APEX**, `product/apex/com.google.android.gmssystem.prodvic.apex`; a three-APK installer would miss this layout.

The pinned Avo source already implements a MindTheGapps installer in its Swift host, with guest overlay mounting, ordered privileged permission installation, restart verification, a persistent commit journal, recovery and removal. GlidePod currently uses the VMM, not that Swift host; its installer does not become available automatically. That code is feasibility/reference evidence, not GlidePod's production implementation.

The new engineering tool prepares the reviewed payload with streaming I/O and can run an installation experiment in a **new disposable store**:

```sh
rtk proxy python3 tools/evaluate-google-apps.py --prepare-only
rtk proxy python3 tools/evaluate-google-apps.py \
  --runtime-store .cache/runtime-upstream-20261105 \
  --candidate .cache/my-new-google-apps-candidate
```

The source store supplies only runtime/renderer registration and resource preferences. No source data disk is copied or changed. Existing candidate directories are rejected. The tool enforces the pinned size/hash, SDK/architecture, expansion/entry limits, safe unique paths, regular files and required privileged allowlists. It never runs recovery scripts from the download, skips recovery persistence, and preserves AOSP Provision rather than adding Google's SetupWizard. The non-tablet phone candidate omits VelvetTitan. Tar preparation streams ZIP members. Guest transfer uses <=64 KiB sync DATA records batched inside the peer's negotiated ADB packet limit, capped at 1 MiB; the temporary batch and transport copy remain bounded. Neither operation loads the entire package into RAM.

The opt-in Rust evaluation starts the fresh guest, disables verity on that instance, mounts its writable overlay, checks for target-file collisions, stages files, copies permissions before APKs/APEX, restores SELinux labels and syncs before restarting. It checks free space on `/data` and `/mnt/scratch` separately: this guest's remount scratch reservation leaves only about 1 GiB free on the 8 GiB data filesystem. The archive goes into the owned `/data` staging area, while extraction uses a private directory on scratch, which has about 6 GiB available. This avoids the full-package archive-plus-extraction disk-full failure observed in the first experiment. A guest-side SHA-256 verifies the complete transferred archive before extraction.

It then checks all three installed/enabled system packages and attempts a managed Play Store task/frame. It stops the candidate on success or failure. **This disposable-instance experiment is not an installer for an existing user profile and has no claim of in-place rollback or interrupted-commit recovery.** A failed candidate is kept for diagnosis rather than silently reset. This audit retained its failure logs and removed only its own stopped failed-candidate disks after inspection.

The large payload also exposed two host adapter issues, now repaired: sync DATA's eight-byte header previously created a second tiny WRTE for each 64 KiB read, and every file transfer shared a fixed 180-second command lifetime. The adapter now batches complete sync records within the negotiated limit and renews a 180-second inactivity deadline while retaining a 30-minute whole-transfer limit. Byte-preserving protocol tests cover 4 KiB, 64 KiB and 1 MiB peer limits. A real 683 MiB transfer completed in 2.044 seconds and matched the guest hash; the successful core candidate's 246 MiB transfer took 0.504 seconds. These are local transfer measurements, not Internet download or total-install timing promises.

On the busy first GMS boot, Package Manager acknowledged the input APK before InputMethodManager discovered its service. `prepare_input` now waits up to ten seconds for the component in `ime list -a -s`, then enables/selects it and reads back the selected method before saving the session marker.

No Google binaries enter source control, the app bundle or the license inventory for redistributed Rust dependencies. A community release digest establishes which download was tested; it is not a Google redistribution license or Google certification. Google's proprietary licensing and device certification requirements still apply to a distributed product. Optional downloading is not automatically a legal exemption. microG is a separate alternative with its own compatibility and signature-spoofing requirements, not a transparent “full GMS” replacement.

## Product installation contract

The intended product action is **Install Google services…**, with the compatible version and download size visible before installation. It must show download, verification, preparation, restart and verification progress; the final states are **Installed, verification pending**, **Ready for sign-in**, **Needs attention**, and **Failed, restored**. Never display “fully supported” based on an APK/package check.

Before exposing this action for user profiles:

1. Resolve Android SDK/ABI from the actual guest and bind the chosen source, digest and guest build to the transaction. Reject a different Android version, a conflicting existing GApps installation, insufficient disk space or an unsupported runtime before mutation. Use the native host download stack so macOS proxy settings apply; qualify PAC and offline/cache failure separately.
2. Take a consistent stopped-profile snapshot or use a separately owned image/data candidate. Journal every changed file and transaction phase durably. Coordinate Start/Stop/Reset/profile switching with the transaction. Install permission allowlists before privileged apps, preserve guest SELinux, and never disable unrelated host security settings.
3. On reboot timeout or interrupted commit, recover through an early guest control channel or restore the snapshot. Do not rely on `sys.boot_completed` to repair a boot-looping guest. Prove disk-full, power/process loss, cancellation and idempotent retry. Offer removal that handles both installed system files and later Play Store updates while preserving unrelated app data.
4. Validate package/signing lineage, the official Google availability API, Google account sign-in, Play Store install/update, and representative Maps/Play Games/billing APIs separately. Keep user credentials in the guest's Google UI. If Google requires custom-ROM device registration, provide an explicit user action; registration is not certification and may not resolve every compatibility restriction. Do not spoof device attestation or promise banking, DRM or anti-cheat acceptance.
5. Bind the installed state to the guest image version and detect incompatible upgrades before boot. Qualify a rootless Personal image and privileged management service separately from this userdebug experiment.

## Push, networking and resource limits

GMS/FCM delivery into Android and forwarding an Android notification to macOS are two different stages. A working local notification does not prove FCM. An FCM token does not prove background message delivery. Forwarding requires both the host app and VM to remain running; stopped/suspended Android cannot receive new notifications through this bridge. Native APNs is not a substitute for third-party Android applications' FCM registrations.

The current Firebase networking guide lists TCP 5228–5230 and 443, long-lived connections and at least 30-minute NAT/SPI timeouts for the messaging connection. It also states that FCM does not support ordinary network proxies and describes VPN bypass behavior. Therefore the [system proxy work](13-vm-network-and-proxy-plan.md) must qualify FCM explicitly. A transparent VMM connector or host tunnel is a separate experiment, not evidence that an Android HTTP proxy works. Required-proxy mode must never silently exempt Google traffic or DNS; report unsupported/unreachable push instead.

Use an original Firebase test app and a test project for registration, foreground/background notification and data messages, device idle, wake, network change, proxy disconnect/reconnect, duplicate suppression and delayed delivery. Verify the controlled sender, Android receipt and macOS receipt as separate timestamps. Test desktop Focus/notification permission denial and VM stop/resume. No Firebase project or test account is supplied in this session, so remote push and sign-in remain external validation gates.

GMS adds resident services, scheduled jobs, storage and network work. Keep the existing 4 vCPU/4 GiB setting as the initial candidate; do not silently increase guest RAM. Compare idle, two Android windows, repeated push, Play Store updates and host pressure with equal workloads. Measure guest PSS, host physical footprint, vCPU work, network-buffer peaks and package/update disk growth after warmup. Installer file transfer and archive preparation stay bounded; production download caches need size/age limits and explicit cleanup after a successful transaction.

## Notification implementation and changes

The existing bridge uses Android `NotificationListenerService`, an explicitly DUMP-protected provider, a bounded host worker and macOS `UNUserNotificationCenter`. Settings → Notifications requests the native permission and enables Android listener access. Stable per-epoch IDs support replacement/removal; host dismissal cancels a dismissible Android notification; clicking opens the corresponding managed app window. The worker handles at most eight changes per second and retains at most the bounded active snapshot, not a content archive.

This audit removes the blanket rejection of every `FLAG_SYSTEM` application. Preinstalled Google and Android apps can post user-facing notifications, so their package flag alone must not suppress forwarding. If package visibility or an uninstall race prevents label lookup, the package ID is used instead of dropping the notification. The helper still excludes itself, other Android users, group summaries, ongoing notifications and secret content; private notifications use their public form or a generic message. UTF-16 truncation no longer splits an emoji surrogate pair, and the host validates app-label length as well as title/body/key limits.

The current click behavior opens the app, not its original notification `PendingIntent`. Inline replies, arbitrary notification action buttons, media/ongoing controls, independent native app icons and background operation after quitting GlidePod remain separate implementation items. Preserve user-intent and epoch/package/notification identity checks when adding them.

## Evidence and remaining gates

- Live baseline inventory confirms all three Google components are absent. Both inventory contract tests pass; installed fixtures remain `present_unverified` and malformed/duplicate inventories are rejected.
- The default core installation passes in a new real guest: Play services `26.11.36`, GSF `17-15555506` and Play Store `51.8.09-29` are enabled system packages after reboot. The managed Play Store window produces a captured **Sign in** page. This is launch/readiness evidence, not an account login or Google API/FCM result. See `candidate-core/after.json`, `result.json` and `play-store.png` in the local evidence directory.
- The full candidate also passes in a separate fresh guest and produces the same Play Store sign-in screen. Its final 683 MiB transfer takes 1.754 seconds and matches the guest hash. Both successful evaluation stores are retained, stopped; neither replaces the original profile.
- Four archive safety tests cover Android 17 APEX, allowlist ordering, SDK mismatch, missing permissions, unsafe paths, symlinks and case collisions.
- The real-guest management suite passes on both the original AOSP runtime and the core GMS candidate, including the new preinstalled system-app notification and dismissal case, public Unicode content, stable updates, private/secret filtering, ordinary-app provider denial, permission denial and listener revocation. The final run uses rebuilt Android helpers and is recorded in `management-with-gms/report.json`; previous reports are preserved. The rebuilt app bundle's CLI independently confirms the installed inventory in `packaged-core-gms.json`.
- The packaged native bridge reports notification authorization `0` (not determined). The desktop provider still returns `cgWindowNotFound` for GlidePod. Native permission/banner/click/dismiss acceptance is not claimed; the user must grant macOS permission before that part can be qualified.
- Workspace format and warning-denying Clippy checks pass, as do 56 ordinary Rust tests (six opt-in tests remain ignored in that ordinary run) and four Python archive tests. The two installation variants and the real-guest management suite are explicitly run separately. Android helpers and the workspace build, the development app is repackaged, and its ad hoc signature passes strict verification. Existing upstream Rust future-compatibility notices remain for `block` and `proc-macro-error2`.
- Candidate installation results and final workspace/package checks are recorded alongside the local logs in `evidence/gms-notifications-2026-11-05/`; the raw evidence is excluded from source publication.

## Primary references

- [Google Play services setup and availability check](https://developers.google.com/android/guides/setup).
- [Google Mobile Services](https://www.android.com/gms/), [uncertified device guidance](https://www.google.com/android/uncertified/warningauto), and [Play Protect certification](https://support.google.com/googleplay/answer/7165974?hl=en).
- [MindTheGapps Android 17 ARM64 release](https://github.com/MindTheGapps/17.0.0-arm64/releases/tag/MindTheGapps-17.0.0-arm64-20261022_132445) and [source revision](https://github.com/MindTheGapps/vendor_gapps/tree/822ff6a805533792ae43a9c3f2a159222da22a6f).
- [Pinned Avo Google apps flow](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/docs/engine.md#google-apps) and `GoogleAppsInstaller.swift`, `GoogleAppsPackage.swift`, `HostAgent.swift` at that revision, inspected as reference.
- [FCM Android requirements](https://firebase.google.com/docs/cloud-messaging/android/get-started) and [FCM network configuration](https://firebase.google.com/docs/cloud-messaging/network-configuration).
- [Play Integrity verdicts](https://developer.android.com/google/play/integrity/verdicts). `MEETS_VIRTUAL_INTEGRITY` is conditional on Google Play Games for PC, not a generic certificate for third-party VMs.
- [microG prerequisites](https://github.com/microg/GmsCore/wiki/Prerequisites).
