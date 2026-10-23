# Android boot and native refinement

Recorded on 2026-10-23. This continues the G0 foundation. It does not mark G0 complete.

## Result

The pinned candidate VMM booted the official ARM64 Cuttlefish build on Apple Silicon through Hypervisor.framework. Android reported `sys.boot_completed=1`; Package Manager returned the framework package; a 720 × 1280 scanout showed the Android Settings application. `reboot -p` shut down the guest and the VMM exited with code 0. This run used software rendering (`--gpu 2d`). It does not establish Android Metal acceleration, game compatibility, independent application windows or production VM management.

The native management center now uses a compact macOS-style toolbar and sidebar, smaller text, grouped settings rows and borderless empty states. The application icon is an original layered Icon Composer document compiled with Apple's tooling. The development bundle includes its system appearance assets.

## Reproducible boot experiment

| Input | Pin / observation |
| --- | --- |
| Host | Apple M5 Max, macOS 27.0.1, 64 GiB unified memory |
| Candidate | Avo revision `99153258763a9ccdf5d4c6de36bd631e4dd0dded` |
| Guest branch | `aosp-android-latest-release` |
| Guest build | `16644040`, created 2026-10-18 |
| Target | `aosp_cf_arm64_only_phone-userdebug` |
| Fingerprint | `generic/aosp_cf_arm64_only_phone/vsoc_arm64_only:17/CP2A.260605.016/16644040:userdebug/test-keys` |
| Android API | 37 |
| Image archive SHA-256 | `a9314183f3cf4d15a0a2121a85f8300ca7990788c52474d322c6fe9320b40802` |
| Guest resources | 4 vCPUs, 4096 MiB, 720 × 1280 display |
| Initial observation | Boot completion observed after about 13.6 seconds |
| Reused instance | Boot completion observed after about 6–7 seconds |
| Rendering | Software, via the candidate's virtio-gpu 2D path |

Timing starts at VMM process creation and ends when the shell query observes completion. Queries are sent every three seconds. These are individual bring-up observations, not a boot-time benchmark or performance promise. The initial report is retained as `evidence/g0/android-boot/first-boot-report.json`; the latest run and logs live beside it.

`engine/cuttlefish-lock.json` fixes artifact names, sizes, API-provided MD5 values and locally measured SHA-256 values. `tools/fetch-cuttlefish.py` fetches from Google's public Android Build API, checks sizes/digests, and records provenance. It also fetches the build's complete source revision manifest. Signed download URLs are not persisted. This was a downloaded official build, not a locally reproduced source build or a publisher-signature verification.

`tools/boot-experiment.py` checks the candidate revision and clean checkout, verifies the image archive, bounds extraction, checks cached image CRCs, signs the separately built VMM for HVF, and runs it under `.cache/android-boot/`. An exclusive experiment lock prevents simultaneous use of this instance. Stale socket paths are retried during startup. The script opens Settings on the test image, requests a BMP from the engine, converts it losslessly to PNG, and requests guest power-off. A deadline terminates and reaps an unresponsive process. Microphone and cameras are disabled; no host shared folder is configured.

Reproduce from the repository root:

```sh
rtk proxy python3 tools/check-upstream.py
rtk proxy python3 tools/fetch-cuttlefish.py
rtk proxy python3 tools/boot-experiment.py
```

Evidence: report (`evidence/g0/android-boot/report.json`, local output), Settings scanout (`evidence/g0/android-boot/android-settings.png`, local output), provenance (`evidence/g0/android-image-provenance.json`, local output).

The candidate remains evaluation-only, outside the application bundle. Its root/engine license mismatch and accurate graphics capability reporting remain adaptation gates. Neither source nor binary images have been published by this work.

## Native interface changes

- One 52-point toolbar with the current page, a 28-point search field and Add APK action; no breadcrumb or duplicate page title.
- A 184-point sidebar, 32-point navigation rows and a small profile/runtime status. Large branding, decorative slogans and the permanent marketing footer are removed.
- Neutral surfaces retain the forest accent. Light and dark modes have matching contrast and control sizes. Input fields no longer cast decorative shadows.
- Preferences use 46-point rows, concise section labels and disclosure of diagnostic details. Large dashboard cards are removed.
- Search supports `⌘F` and the existing `⌘K`, a clear button, Escape and a specific no-results state. Search focus leaves hidden pages and modal content.
- APK review uses a compact filename/package header and version, architecture, API and size rows. The sheet blocks background mouse interaction. File pickers cannot be duplicated while one is pending; a second file cannot silently replace an open review.
- Actual package state remains empty. The UI never presents the separate experiment as an installed, supported product runtime.

Source: `crates/glidepod-ui/src/views.rs` for presentation and `main.rs` for actions/state. The browser prototype is retained for historical design context; the native app is the current implementation.

## macOS 27 application icon

The primary source is `assets/GlidePod.icon`, with two original SVG groups. Overlapping sage and ivory panels describe the product without letters, a mascot or borrowed Apple artwork. The forest background preserves the existing identity.

Icon Composer supplies the system mask, depth, glass and lighting. `tools/export-icons.py` explicitly selects design generation 27 and exports Default, Dark, Clear Light/Dark and Tinted Light/Dark, plus small-size previews. `tools/package-dev.py` compiles the `.icon` document with `actool`, includes `Assets.car` and the ICNS fallback, and merges the generated icon metadata into Info.plist. Xcode 27 is the packaging requirement for this source.

References: [Apple Icon Composer](https://developer.apple.com/icon-composer/), [Icon Composer workflow](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer), [app icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons). This follows the platform's rendering workflow; it is not an Apple certification or endorsement.

## Validation and next gates

Validation artifacts live in `evidence/g0/` and `evidence/ui-refinement/`. Formatting, Clippy with warnings denied, all 23 workspace tests, CLI contracts, HVF/Metal host probes, icon bundle resources and ad-hoc signature verification passed. Native visual checks passed for navigation, search/clear/Escape, APK file selection/review, modal isolation, 820 × 560 minimum window layout, light/dark appearance and packaged helper checks. The final binary hashes are in `evidence/g0/validation.json`.

Screenshots: Apps (`evidence/ui-refinement/01-apps.jpg`, local output), search (`evidence/ui-refinement/02-search.jpg`, local output), APK sheet (`evidence/ui-refinement/03-apk-sheet.jpg`, local output), minimum window (`evidence/ui-refinement/04-minimum-window.jpg`, local output), host checks (`evidence/ui-refinement/05-host-checks.jpg`, local output), dark settings (`evidence/ui-refinement/06-dark-settings.jpg`, local output). These are native app captures, not browser mockups. A purple control badge visible in some captures belongs to the computer-use session, not GlidePod.

The accessibility tree exposed by the current GPUI build is sparse; VoiceOver support is not established. macOS 15/26 runtime and Icon Composer appearance behavior on those versions remain untested. Cargo reports existing future-incompatibility notices for `block 0.1.6` and `proc-macro-error2 2.0.1`; current build and Clippy checks pass.

Next runtime work is to qualify the guest GPU path and honest Vulkan capabilities, then connect an isolated supervised runtime and display transport to the native app. Multi-window/task identity, input/IME, APK install sessions, launcher bundles, root enforcement and widgets remain implementation work. A single software-rendered Android display does not satisfy the agreed G0 multi-window and GPU gates.
