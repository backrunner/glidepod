# 3D evaluation, app storage and notification permissions

Date: 2026-10-23. Scope: the registered experimental Developer runtime on Apple Silicon. This extends records 04 and 05. It does not qualify arbitrary games, a rootless Personal image or a public release.

[Record 07](07-gles3-metal.md) supersedes the GLES 3 blocker for a separately built and patched MoltenVK 1.4.2 candidate. The original 1.4.0 measurements below remain unchanged historical evidence. [Record 08](08-native-vulkan.md) subsequently adds direct Vulkan correctness and performance measurements.

## What the app provides

The app library now has an **Info** action. App information shows the package/version, program size, user data, cache, total size and requested Android runtime permissions. **Clear Cache** preserves app data. **Clear Data…** asks for confirmation and closes the app window before deleting Android files/accounts/settings; the package stays installed. System apps, shared-UID apps and GlidePod infrastructure helpers cannot be mutated through these controls. **Android Settings…** opens the selected app's Android settings in an independent window, including notification channels and special access.

Storage comes from `StorageStatsManager`, not APK archive size or guessed host disk usage. Android includes cache in `dataBytes`; the UI subtracts cache when showing user data and adds only app+data for the total. Shared-UID statistics are labeled as potentially aggregated. The Android page reports available/total bytes on the guest data filesystem. These are separate from the Mac's allocated VM disk size; deleting Android files does not automatically compact the host disk image.

Each permission change is restricted to an explicitly requested runtime permission, rejects system/policy-fixed permissions, and reads back the resulting grant. Revoking a permission closes the native app window because Android may kill the app process. Granting Android Camera/Microphone/Location does not create a host device bridge; those inputs remain unavailable until their separate integration is implemented.

On the pinned Android 17 image, shell-UID `pm clear --cache-only` is silently ignored without `INTERNAL_DELETE_CACHE_FILES`, leaving its observer waiting. The Developer adapter uses verified **guest** root for this package-manager operation, with a 20-second guest timeout. It never deletes raw directories or trims other apps' caches. A rootless production image needs a privileged, platform-signed management service. This does not request macOS root and does not turn the Developer CLI root-shortcut preference into an isolation boundary.

## 3D measurements

The original `dev.glidepod.benchmark` APK renders animated, textured, lit, depth-tested 3D toruses through a GLES 2 context. Each object has 1,024 triangles; the matrix uses 1, 16 and 64 objects at 720×1080, plus 16 objects at 1080×1920. Shader locations and geometry are cached. Every run warms up for two seconds and then collects 180 frames; the evaluation script repeats every case twice.

Normal runs measure CPU submission and consecutive draw-start intervals, including swap/backpressure between frames. A separate synchronized case inserts `glFinish` after each frame and measures submission plus completion. It intentionally removes normal GPU pipelining. Neither timing is a hardware GPU timer query. A real ES 3 EGL configuration/context probe is also recorded; context creation alone would not qualify GLES 3 correctness.

The host consumer records frame-copy timing and the same RGBA-to-BGRA conversion used by the GPUI window, including sample counts and percentiles. The no-consumer case disables this Rust consumer; it does **not** disable the upstream renderer's readback or Android composition. Reports include all timing samples, p50/p95/p99/max, intervals over 16.67/33.33 ms, workload/resolution, GL vendor/renderer/version/extensions, a nonempty pixel check, VM epoch, build profile, host hardware, VMM CPU consumption and end-of-run RSS. Captures come from the real guest frame transport.

The first full consumer run exposed a real host bottleneck: the unoptimized development build spent roughly 15–19 ms converting 720×1080 pixels and about 41 ms at 1080×1920 (p95). The UI and diagnostic now share the same conversion function, and `glidepod-runtime` is optimized in the development profile while retaining debug assertions. The unoptimized runs are preserved under `metal-unoptimized`. The final matrices use the optimized runtime: Metal conversion p95 falls to 1.0–1.6 ms at 720×1080 and 2.4–2.5 ms at 1080×1920.

### Measured results

Host: Apple M5 Max, macOS 27.0.1, 64 GiB RAM. Each guest has four vCPUs and 4 GiB RAM. Metal and Software ran sequentially, with no concurrent builds or other guest tests. All 24 runs completed their sample count, produced nonempty rendered pixels, and returned no GL error. Values below span the two repetitions; frame p95 is the range of per-run percentiles, not a pooled percentile.

| Workload | Renderer | Mean FPS | Frame interval p95 (ms) | VMM CPU (%) |
| --- | --- | --- | --- | --- |
| 16 objects, 720×1080 | Metal | 59.69–59.98 | 22.50–22.84 | 54–63 |
| 16 objects, 720×1080 | Software | 59.99–60.00 | 20.14–20.17 | 116–117 |
| 64 objects, 720×1080 | Metal | 59.64–59.94 | 20.84–22.52 | 60–64 |
| 64 objects, 720×1080 | Software | 59.97–60.00 | 18.54–19.91 | 171–173 |
| 16 objects, 1080×1920 | Metal | 59.96–60.01 | 22.23–22.70 | 60–61 |
| 16 objects, 1080×1920 | Software | 59.97–60.00 | 20.72–20.80 | 133–137 |

CPU is the entire VMM process over each roughly seven-second invocation, including launch/warmup, Android CPU work and rendering; 100% means one logical core. It is not GPU utilization or steady-state draw-only CPU. End-of-run VMM RSS ranges from 4,652–4,672 MiB for Metal and 4,212–4,644 MiB for Software across the matrix; RSS includes guest memory and is not VRAM usage.

Both renderers meet the scene's approximately 60 Hz pacing, so this workload does not establish maximum GPU throughput or a Metal FPS advantage. Metal uses less VMM CPU here, but its frame p95 is worse. One heavy Metal run contains a roughly 50 ms interval; average FPS alone would hide it. Removing the Rust consumer retains approximately 60 FPS on both renderers. The synchronized diagnostic also remains near 60 FPS and does not provide GPU-only timing.

The real GL strings identify Metal as ANGLE → gfxstream Vulkan → Apple M5 Max, and Software as ANGLE → SwiftShader. **Metal has no ES 3 EGL configuration on this pinned stack.** Software successfully creates an ES 3 context and reports ES 3.1; that is a context probe, not an ES 3 conformance test. Modern games requiring GLES 3 remain an explicit compatibility blocker for the Metal path. Do not advertise general Android game support from these results.

Raw results: Metal matrix (`evidence/g4/3d/metal/report.json`, local output), Software matrix (`evidence/g4/3d/software/report.json`, local output), heavy scene capture (`evidence/g4/3d/metal/heavy-720-1.png`, local output), 1080p capture (`evidence/g4/3d/metal/medium-1080-1.png`, local output), and measured APK payload hashes (`evidence/g4/3d/measured-apk.json`, local output). Future performance acceptance needs a workload that exceeds the refresh ceiling, hardware GPU timing, native-window presentation measurements and representative licensed game workloads, followed by sustained thermal runs.

The Graphics page exposes **Run 3D Check** and the last result. The CLI and evaluation script provide the fuller matrix:

```sh
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-gpu-test --json benchmark --objects 64 --samples 180
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-gpu-test --json benchmark --objects 64 --finish-each-frame
rtk proxy python3 tools/evaluate-3d.py --data-dir .cache/runtime-gpu-test --samples 180 --repeat 2
```

Run Metal and Software sequentially, with only one evaluation VM active. The script refuses stores outside this project's `.cache` and never changes the user's runtime preferences. `evidence/g4/3d/metal/` and `software/` hold the final matrices. Earlier synchronized trials are explicitly retained as a baseline, not mixed with the final normal-pipeline results.

Native GPUI texture upload/composition, physical display latency, power/thermal endurance, Vulkan app performance, ASTC correctness and representative game compatibility remain separate gates. A short synthetic scene cannot establish a game's frame rate or a sustained thermal limit.

## Notification integration

**Settings → Notifications → Android app notifications** is off by default. Enabling it explicitly requests macOS notification permission, installs the original Android listener helper, grants Android notification access and saves the preference. Disabling it revokes the guest access and clears mirrored notifications. If Android is stopped, the preference can still be disabled and revocation is reconciled at the next start. The macOS permission itself remains managed by System Settings.

The Android `NotificationListenerService` exposes a bounded snapshot through a private provider. The provider explicitly requires `android.permission.DUMP` in its `call` method; ordinary applications cannot read or dismiss notifications through it. The host uses Android's `content` shell client, which supplies the correct shell attribution. GlidePod keeps no additional notification archive and does not send notification content over the network; macOS manages the delivered notification history.

The relay follows the app lifetime, including when the management window is closed but Android app windows remain. It requires a running matching VM epoch, enabled forwarding and macOS authorization. It polls once per second, keeps stable identifiers for updates, removes cancelled notifications, bounds delivery to eight changes per cycle, and imports initial Android history quietly. Secret notifications are omitted. Private notifications use their public version or a generic placeholder. Ongoing notifications, group summaries, system apps and other Android users are excluded. Android ranking/interruptibility controls passive versus active delivery; macOS user settings still govern presentation.

Clicking opens the Android app in its managed window. Dismissing a clearable native notification cancels the corresponding Android notification. Callback identity, package and VM epoch must match the live relay state. This first bridge does not invoke arbitrary notification `PendingIntent` deep links, expose reply/actions, forward notification sounds, provide independent app icons/Dock identities, or create remote push/FCM support. It mirrors notifications that Android actually receives.

## Verification and limits

The opt-in management suite uses only the original fixture in a disposable guest. It verifies storage statistics, preservation of private files during cache clearing, data deletion without uninstalling, system operation guards, runtime permission changes, notification permission denial, listener grant/revoke, Unicode content, stable update identity, privacy filtering, dismissal and rejection of an ordinary app's provider access.

```sh
rtk proxy env GLIDEPOD_TEST_STORE=/absolute/project/.cache/runtime-gpu-test cargo test -p glidepod-runtime --test management -- --ignored --nocapture
```

The shared-UID/system/helper mutation guards and cache accounting also have focused unit coverage.

The Android evidence is management/report.json (`evidence/g4/management/report.json`, local output). Native notification banners, click/dismiss callbacks and macOS permission-dialog interaction require an operable desktop and are not claimed as physical UI passes. Final host checks, package hashes, regression evidence and remaining manual gates are recorded in bundle-validation.json (`evidence/g4/bundle-validation.json`, local output).

Final validation passed formatting, strict Clippy, 46 workspace tests, CLI error/confirmation contracts, Android helper compilation, packaging and deep/strict ad-hoc signature verification. Four opt-in real-guest suites passed after the rebuild: management, continuous window input, host focus-lease routing and app lifecycle. The rebuilt benchmark's manifest, resources and DEX are byte-identical to the measured APK payload; ZIP/signing hashes differ and both hashes are retained. The dependency notice inventory still matches the resolved Cargo graph.

The updated `dist/GlidePod.app` was relaunched and its process path and packaged binary hashes verified. The default user store remains on Personal, stopped, with forwarding disabled and its preferences unchanged. The native UI inspection returned `cgWindowNotFound`; no button or banner interaction is claimed. Both disposable evaluation VMs were stopped and their engine processes were confirmed absent; see cleanup.json (`evidence/g4/cleanup.json`, local output).

## Sources

- [Android StorageStats](https://developer.android.com/reference/android/app/usage/StorageStats) and [StorageStatsManager](https://developer.android.com/reference/android/app/usage/StorageStatsManager).
- [AOSP PackageManager shell implementation](https://android.googlesource.com/platform/frameworks/base/+/main/services/core/java/com/android/server/pm/PackageManagerShellCommand.java).
- [Android NotificationListenerService](https://developer.android.com/reference/android/service/notification/NotificationListenerService).
- [Apple UNUserNotificationCenter](https://developer.apple.com/documentation/usernotifications/unusernotificationcenter) and [notification delegate](https://developer.apple.com/documentation/usernotifications/unusernotificationcenterdelegate).
- [Android GLSurfaceView.Renderer](https://developer.android.com/reference/android/opengl/GLSurfaceView.Renderer) and [Khronos OpenGL ES specifications](https://registry.khronos.org/OpenGL/index_es.php).
