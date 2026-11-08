<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/glidepod-icon-dark.png">
    <img src="assets/brand/glidepod-icon-light.png" width="128" height="128" alt="GlidePod's two overlapping glass packages">
  </picture>
</p>

<h1 align="center">GlidePod</h1>

<p align="center">Android apps, in their own Mac windows.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-315E4B" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/platform-Apple_Silicon-617566" alt="Platform: Apple Silicon">
  <img src="https://img.shields.io/badge/status-experimental-8B765C" alt="Status: experimental">
</p>

<p align="center">
  <a href="#what-works-today">Features</a> ·
  <a href="#build-from-source">Build</a> ·
  <a href=".agents/development/runtime.md">Runtime setup</a> ·
  <a href=".agents/README.md">Engineering notes</a> ·
  <a href="CONTRIBUTING.md">Contribute</a>
</p>

GlidePod is an open-source Android environment for Apple Silicon Macs, built with **Rust, GPUI and Cuttlefish ARM64 guests**. Install an APK, open it in a separate macOS window, and manage its storage, permissions and runtime from one place.

> [!IMPORTANT]
> **Developer preview.** The management app and guest integration are implemented, but a runtime must be prepared separately. There is no ready-to-install, notarized release or bundled Android image yet. Broad game compatibility and production reliability remain unqualified.

## What works today

| Area | Current implementation |
| --- | --- |
| **Apps** | APK install, update and removal; searchable catalog; APK file association; automatic Finder and Spotlight app entries. |
| **Windows** | Independent Android displays and GPUI windows, separate frame/input workers, and per-app macOS Dock entries with Show Window and Quit. |
| **Input** | Unicode text, system clipboard bridges for text/images/files, held keyboard events, touch/mouse modes, macOS trackpad scrolling/pinch/rotation and native GameController mapping. |
| **Files** | Window-owned Mac file/image panels, compatible app drops, Save to Mac and automatic completed public-download export; no shared-directory setup. |
| **Management** | Developer VM start/stop and resources; app storage, cache/data cleanup and Android runtime permissions. |
| **Menu bar** | VM state, open-app count/list, live VM CPU and memory, assigned resources, and quick access to app windows. Closing the last window keeps the tray available. |
| **Desktop integration** | Live Android widget panels and opt-in notification forwarding. Native WidgetKit support is still planned. |
| **Graphics** | Software baseline and experimental gfxstream → MoltenVK / KosmicKrisp → Metal paths; dedicated GLES 3 and Vulkan correctness/performance fixtures. |

Real guest checks have exercised app lifecycle, independent displays/input, storage, permissions, GLES 3 and direct Vulkan. Physical trackpad taps, scrolling, pinch in/out and rotation reached Android in an unlocked native session. Physical keyboard/IME, broader mouse/controller behavior, current native notification interactions, Launchpad indexing and end-to-end presentation latency still need broader on-device validation. See the [implementation records](.agents/README.md) for the scope of each result.

Host file integration is implemented for the pinned Developer image. Drops use supported app Share actions; custom galleries and arbitrary drop targets remain app-dependent. Native Open/Save panels, cancellation and menu-based text clipboard transfer have been exercised in an unlocked session. Physical Finder dragging and broader keyboard interaction remain unqualified; Photos-library selection is not yet included. See the [implementation and acceptance record](.agents/implementation/17-host-files-and-clipboard-plan.md).

<details>
<summary><strong>Native management center preview</strong></summary>

<p align="center"><img src=".agents/media/native-apps.jpg" width="960" alt="Native GlidePod Apps page with sidebar, search field, Add APK action and setup empty state"></p>

An actual capture of the early native GPUI management center, before a runtime was registered. The purple badge in the title bar belongs to the capture session. This is not a screenshot of Android apps running in separate windows. The separate [interactive browser prototype](.agents/prototype/README.md) is a simulated design reference.

</details>

## How it works

The Android guest runs under an external Rust VMM using Apple's Hypervisor.framework. GlidePod owns the app catalog, lifecycle, per-app displays and macOS windows. Cuttlefish supplies the Android guest; its Linux host launcher is not the macOS runtime.

```text
Android app
  ├─ OpenGL ES → guest ANGLE ─┐
  └─ Vulkan ─────────────────┴─→ gfxstream → MoltenVK / KosmicKrisp → Metal
                                  │
                            frame readback
                                  ↓
                         per-app GPUI window
```

The current transport reads frames back to the host. Shared IOSurface presentation is planned. GPUI renders the Mac interface; the separate guest graphics stack accelerates Android. **Automatic currently uses Software**; Metal requires an explicitly registered external renderer. See [runtime setup](.agents/development/runtime.md) and the [graphics design](.agents/research/03-graphics-and-games.md).

The optional KosmicKrisp candidate requires Apple Silicon and macOS 26 or later. **Graphics → Vulkan driver** retains both registered drivers and allows selection while Android is stopped; it shows the live driver and next-start selection separately. Existing registrations retain MoltenVK. See the [driver comparison](.agents/implementation/23-graphics-drivers-and-metal.md) and [uncapped pressure tests and direct GLES → Metal experiment](.agents/implementation/24-graphics-pressure-and-driver-settings.md). The subsequent [source-built direct GLES Metal candidate](.agents/implementation/25-direct-gles-metal-correctness.md) adds indexed-topology and cross-context fixes with a shared host/Android regression matrix; it remains a separate ES 3.0 experiment. The [readback follow-up](.agents/implementation/26-metal-readback-tail.md) reduces shared graphics-thread stalls through bounded strip reads and covers multi-window lifecycle regressions. [High refresh display support](.agents/implementation/27-high-refresh-displays.md) adds 60/90/120 Hz choices and notification-driven frame delivery, with separate guest and host measurements.

## Build from source

Development currently requires **Apple Silicon, Xcode 27, Rust and Python 3.12+**. Validation has used macOS 27.0.1 and Rust 1.99.1. The configured macOS 15 deployment target and Rust 1.90 minimum still need separate validation.

```sh
git clone https://github.com/backrunner/glidepod.git
cd glidepod
python3 tools/prepare-android-tools.py
cargo fetch --locked --target aarch64-apple-darwin
python3 tools/license-inventory.py
python3 tools/package-dev.py
open dist/GlidePod.app
```

This produces a local, ad-hoc signed development app. It downloads compile tools into `.cache/` and builds the original Android helpers; it does not install an Android system image. Follow the **[build guide](.agents/development/building.md)** for checks and prerequisites, then **[prepare the runtime](.agents/development/runtime.md)** to run APKs.

The management center can be explored without a guest. `⌘O` opens an APK, `⌘F` focuses search and `⌘R` refreshes. The companion CLI is available at `dist/GlidePod.app/Contents/MacOS/glidepodctl`; use `--help` to see commands.

Settings offers Smart start/stop and Always on. A lightweight background service keeps the VM and notification relay independent of open windows. Automatic memory reclamation requires the patched evaluation engine built with `python3 tools/build-memory-engine.py --data-dir PATH` against a stopped runtime. See [behavior, validation and limitations](.agents/implementation/adaptive-background-runtime.md).

Google services can be installed optionally in the pinned Developer image, turned off with data preserved, re-enabled, or removed and reinstalled. Minimal built-in apps is on by default: it reversibly disables optional phone, clock, contacts and demo apps while retaining Android's core services and storage providers. See [the package list and verification limits](.agents/implementation/optional-google-and-minimal-apps.md).

Settings also offers Off/Follow Mac/virtual location and Shared/Bridged network attachment. Virtual coordinates use the guest GPS path with normal Android permissions. Mac location needs macOS permission; bridged networking needs an authorized vmnet runtime and remains unqualified on a real LAN. Build the evaluation adapter with `python3 tools/build-memory-engine.py --host-services`; see [behavior and acceptance gates](.agents/implementation/location-and-network-modes.md). Shared host routing does not automatically apply system HTTP/SOCKS proxies.

## Next milestones

- A reproducible, redistributable runtime and a rootless Personal image.
- Async block I/O and better frame pacing, with persistence and scheduling tests.
- Lower-copy presentation and sustained GPU/game measurements.
- Broader physical input, accessibility and native notification validation.
- Native WidgetKit integration, signed/notarized downloads and a wider macOS test matrix.

Relative mouse capture, controller rumble and multiple players are also pending. Google Play services are not bundled; optional installation does not establish Google sign-in, certification or remote push compatibility. DRM-dependent apps, anti-cheat compatibility and x86 Android translation are not provided. Advertised API versions and passing synthetic checks are not graphics conformance or game certification.

## Explore the project

- **[Architecture and research](.agents/README.md)** — decisions, references and the interactive design prototype.
- **[Independent windows and input](.agents/implementation/05-independent-windows-and-input.md)** — display ownership, input routing and acceptance boundaries.
- **[Touch and trackpad gestures](.agents/implementation/18-touch-and-trackpad-input.md)** — coordinate checks, native gesture mapping, game-input limits and Android recognition evidence.
- **[GLES 3](.agents/implementation/07-gles3-metal.md)** and **[Vulkan](.agents/implementation/08-native-vulkan.md)** — original workloads, renderer patch and measured limits.
- **[Frame pacing investigation](.agents/implementation/09-vulkan-frame-pacing.md)** — what the traces establish and what remains unresolved.
- **[Icon source](assets/README.md)** — the editable layered package mark in Icon Composer.
- **[Contributing](CONTRIBUTING.md)** — build checks, evidence expectations and dependency policy.

## License

Original GlidePod code, documentation and artwork are licensed under **[Apache-2.0](LICENSE)**. It permits use, modification and redistribution, including commercial use, and includes an express contributor patent grant under its terms.

Dependencies keep their own licenses. See [NOTICE](NOTICE), [third-party notices](THIRD_PARTY_NOTICES.md) and the [dependency inventory](licenses/dependency-inventory.json). External VMM/renderer binaries, Android images, SDK archives and third-party APKs are not included in this source repository. Binary distribution requires a separate component and notice review.

GlidePod is an independent project and is not affiliated with Apple, Google or the Android Open Source Project.
