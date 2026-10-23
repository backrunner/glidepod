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
| [Direct Android Vulkan](implementation/08-native-vulkan.md) | Original NDK workload, Vulkan 1.3 checks, synchronization validation, independent displays and Metal/Software measurements |
| [Vulkan frame pacing](implementation/09-vulkan-frame-pacing.md) | Perfetto and host frame markers distinguish swapchain queue backpressure from display interruption; corrected measurement scope |
| [Asynchronous block I/O plan](implementation/10-async-block-io-plan.md) | Follow-up VMM scheduling fix with ordering, persistence and reset gates; not implemented |
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

- [Independent windows and device input](implementation/05-independent-windows-and-input.md) — continuous keyboard/touch/mouse/controller input, focus cleanup and dual-app Software/Metal verification; physical input acceptance remains pending.
