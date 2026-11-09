# GlidePod design package

For a fresh checkout, start with the [build guide](development/building.md) and [external runtime setup](development/runtime.md). The research and implementation records below are dated engineering notes, not a list of release-qualified features. Raw `evidence/` paths refer to [local validation outputs](../evidence/README.md); those files are not included in the public repository. The current source publication and license scope are described in the [project README](../README.md).

Research date: **2026-10-23**. Status: **experimental APK installation, app catalog, independent displays and frame workers, GPUI app windows, keyboard/touch/mouse/controller input, Android widget panels and Metal integration**. Production qualification and complete game/fusion support remain open. See [the feature matrix](implementation/04-apps-windows-and-metal.md) and [current window/input record](implementation/05-independent-windows-and-input.md) for implemented behavior, real guest evidence and remaining gates.

GlidePod brings Android applications onto the Mac as individual windows, with an APK installer, a small management center, and normal application entries. The proposed implementation uses Rust, GPUI, Cuttlefish-derived Android images, and Apple's GPU through gfxstream, MoltenVK, and Metal.

## Recommended direction

- Target **Apple Silicon and macOS 15+** first; test macOS 15, 26, and 27 separately. This baseline is a proposal, subject to the dependency and hardware gates.
- Boot ARM64 Android directly with a Rust VMM on **Hypervisor.framework**. Use Cuttlefish's guest device model and build infrastructure; do not assume its Linux `cvd` host tooling runs on macOS.
- First evaluate a pinned, audited adaptation of Avo's Rust VMM (root Apache-2.0; engine manifests declare BSD-3-Clause, pending clarification). Keep GlidePod's runtime protocol independent. Its recent source is useful feasibility evidence, not a production reliability guarantee.
- Render GLES through **guest ANGLE → Vulkan → gfxstream → host MoltenVK → Metal**; Vulkan applications enter at Vulkan. Start with a measurable readback presentation path, then qualify shared IOSurface presentation. GPUI's GPU rendering does not accelerate Android by itself.
- Give every Android task a managed display and a real Mac window. Use per-application launcher bundles for Dock, app switching, Spotlight, and Launchpad where available.
- Use **Apache-2.0 for new project code**; preserve all dependency licenses, source obligations, and notices. Distribute outside the App Store with Developer ID signing and notarization.
- Support Android widgets in live desktop panels; provide a separate SwiftUI/WidgetKit extension for native status, launch actions, and cached previews.

## Read in this order

| File | Purpose |
| --- | --- |
| [01 — Feasibility](research/01-feasibility.md) | Evidence, alternatives, scope and non-negotiable unknowns |
| [02 — Architecture](research/02-architecture.md) | Processes, Cuttlefish changes, windowing, protocols, lifecycle and security |
| [03 — Graphics and games](research/03-graphics-and-games.md) | Metal path, synchronization, performance gates and compatibility |
| [04 — Product and integration](research/04-product-and-integration.md) | Installer, native integration, root, widgets and interaction specification |
| [05 — Licensing and distribution](research/05-licensing-and-distribution.md) | License recommendation, dependency boundaries and release requirements |
| [06 — Implementation plan](research/06-implementation-plan.md) | Sequenced work, deliverables, acceptance and fallback decisions |
| [Sources](research/sources.md) | Primary sources, inspected revisions and evidence limits |
| [Prototype guide](prototype/README.md) | Running and reviewing the interactive design |
| [UI refinement](prototype/ui-refinement.md) | Refined controls, reduced decorative copy and original graphical icon |
| [Validation](validation.md) | Actual checks performed on this design package |
| [Native foundation](implementation/01-g0-foundation.md) | Initial implementation and host diagnostics |
| [Android boot and native refinement](implementation/02-android-boot-and-native-refinement.md) | Current boot evidence, compact native UI and macOS 27 icon |
| [Experimental runtime management](implementation/03-experimental-runtime-management.md) | Developer VM lifecycle, process ownership, slot correction and current verification limits |
| [Apps, windows and Metal](implementation/04-apps-windows-and-metal.md) | Current implementation, feature matrix, guest acceptance and remaining gates |
| [Independent windows and device input](implementation/05-independent-windows-and-input.md) | Separate frame/input workers, focus leases, continuous keyboard/pointer events, native controller mapping and real guest acceptance |
| [3D, storage and notifications](implementation/06-3d-storage-and-notifications.md) | Real guest 3D workloads and frame transport costs, app storage/permissions, opt-in notification integration and evidence limits |
| [GLES 3 Metal renderer](implementation/07-gles3-metal.md) | Source-built MoltenVK, dynamic fan fix, 13 targeted correctness checks, reproducible renderer and performance evidence |
| [Graphics drivers and Metal](implementation/23-graphics-drivers-and-metal.md) | Current software/GPU paths, optional KosmicKrisp, driver comparison and remaining readback costs |
| [Graphics pressure and driver settings](implementation/24-graphics-pressure-and-driver-settings.md) | Uncapped GLES/Vulkan measurements, direct GLES Metal prototype, retained driver choices and correctness gates |
| [Direct GLES Metal correctness and pressure](implementation/25-direct-gles-metal-correctness.md) | Pinned source builds, topology/context/presentation fixes, host/guest regressions and controlled pressure comparison |
| [Metal readback and completion tails](implementation/26-metal-readback-tail.md) | Display-readback tracing, bounded strip reads, packing regressions and isolated performance comparison |
| [High refresh displays](implementation/27-high-refresh-displays.md) | 60/90/120 Hz configuration, frame notifications, guest and host delivery measurements |
| [UI density and search alignment](implementation/28-ui-density-and-search.md) | Compact copy and control spacing, centered search line layout, theme fixes and native verification limits |
| [macOS interface polish](implementation/30-macos-ui-polish.md) | Neutral shared controls, grouped navigation, system appearance, measured cleanup and native review evidence |
| [Direct Android Vulkan](implementation/08-native-vulkan.md) | Original NDK workload, Vulkan 1.3 checks, synchronization validation, independent displays and Metal/Software measurements |
| [Vulkan frame pacing](implementation/09-vulkan-frame-pacing.md) | Perfetto and host frame markers distinguish swapchain queue backpressure from display interruption; corrected measurement scope |
| [Asynchronous block I/O plan](implementation/10-async-block-io-plan.md) | Follow-up VMM scheduling fix with ordering, persistence and reset gates; not implemented |
| [Native UI and functional audit](implementation/11-native-ui-audit.md) | Prototype comparison, real native interactions, task-lifecycle repair, guest regressions and measured workspace cleanup |
| [Upstream and frame performance](implementation/12-upstream-and-frame-performance.md) | November dependency audit, bounded BGRA presentation, host microbenchmark and prioritized runtime follow-up |
| [Overall performance audit](implementation/22-performance-audit.md) | Measured provider/CPU/frame improvements, bounded observation channels, export parsing and remaining bottlenecks |
| [VM networking and proxies](implementation/13-vm-network-and-proxy-plan.md) | Implemented system proxy inventory; planned VM-wide proxy enforcement, DNS/UDP coverage and network memory budgets |
| [Google services and notifications](implementation/14-google-services-and-notifications.md) | Live GMS prerequisites, isolated optional-install candidate, notification coverage fix and separate sign-in/FCM/native acceptance gates |
| [Google services product workflow](implementation/15-google-services-product-and-system-notifications.md) | Transactional optional installation, removal/recovery and Android system notification delivery |
| [Source review](implementation/16-source-review.md) | Review corrections, submission checks and explicit validation boundaries |
| [Host files, downloads and clipboard](implementation/17-host-files-and-clipboard-plan.md) | Implemented Developer-image file selection, URI delivery, download export and clipboard bridges; guest evidence and native acceptance boundaries |
| [Touch and trackpad input](implementation/18-touch-and-trackpad-input.md) | Touch coordinate/release accuracy, native scroll/pinch/rotation, game-input limits and real Android gesture recognition |
| [macOS menu bar](implementation/21-macos-menu-bar.md) | Window-independent VM monitoring, measured host process resources, app switching and native acceptance boundaries |
| [Adaptive memory and background runtime](implementation/adaptive-background-runtime.md) | Smart start/stop, persistent background ownership, guest-aware reclamation, measured lifecycle results and remaining GMS acceptance |
| [Resident memory audit](implementation/resident-memory-audit.md) | Host/guest alias accounting, real backing measurements, controlled optimization experiments and bounded idle compaction |
| [Optional Google services and minimal apps](implementation/optional-google-and-minimal-apps.md) | Data-preserving GMS off/on, transactional reinstall, reversible built-in app trimming and real VM acceptance |
| [Location and network modes](implementation/location-and-network-modes.md) | Mac and virtual GPS sources, Shared/Bridged attachment, engine adaptation and explicit permission/runtime acceptance gates |
| [Icon Composer reconstruction](design/icon-composer/README.md) | Selected B design rebuilt as editable vector layers, clean background, appearance previews and development bundle verification |

## Open the prototype

Open [prototype/index.html](prototype/index.html) directly in a browser. It is self-contained apart from its adjacent CSS, JavaScript and SVG assets, uses no remote assets, and requires no build. For a local HTTP preview, run from the project root:

```sh
rtk proxy python3 -m http.server 4318 --bind 127.0.0.1 --directory .agents
```

Then visit `http://127.0.0.1:4318/prototype/`. All VM state, APK metadata, Android content, FPS, and storage figures are illustrative. Selecting a local APK reads only its name and size; its bytes are neither parsed nor uploaded. The prototype makes no operating-system changes.

## First engineering decision

Run the G0 feasibility sprint before committing to a release date. The critical experiment is two independently focused Android applications, rendered by the Mac GPU, in separate Mac processes and windows, with accurate feature reporting and a reproducible redistributable build. A full-screen Android demo alone does not pass.

## Implementation status

The native Rust/GPUI foundation now exists. The subsequent [Android boot and native refinement record](implementation/02-android-boot-and-native-refinement.md) documents the real candidate boot and the current native design. The HTML prototype is a historical, simulated design artifact. G0 is not complete.

- [Experimental runtime management and slot correction](implementation/03-experimental-runtime-management.md) — real Developer VM start/stop, process ownership and validation.

- [Apps, windows and experimental Metal](implementation/04-apps-windows-and-metal.md) — installation, per-app displays, Unicode input, launcher bundles and live widget hosting.

- [Independent windows and device input](implementation/05-independent-windows-and-input.md) — continuous keyboard/touch/mouse/controller input, focus cleanup and dual-app Software/Metal verification; [physical trackpad forwarding](implementation/18-touch-and-trackpad-input.md#physical-trackpad-acceptance-2026-11-08) is verified, while broader hardware input acceptance remains pending.

- [Google services product workflow and Android system notifications](implementation/15-google-services-product-and-system-notifications.md)
