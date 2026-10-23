# Research source register

Accessed 2026-10-23. Primary documentation and project-owned source were preferred. Dynamic `main` links can change; pin their revisions in G0. “Inspected” means read during this research, not compiled, executed or comprehensively audited.

| ID | Primary source | Used for / limits |
| --- | --- | --- |
| S01 | [Cuttlefish overview](https://source.android.com/docs/devices/cuttlefish) | Virtual-device scope |
| S02 | [Cuttlefish setup](https://source.android.com/docs/devices/cuttlefish/get-started) | Official KVM/Linux host setup |
| S03 | [Cuttlefish GPU acceleration](https://source.android.com/docs/devices/cuttlefish/gpu) | gfxstream, Virgl and SwiftShader distinction |
| S04 | [Android Cuttlefish host utilities](https://github.com/google/android-cuttlefish/blob/main/README.md) | Linux host utility scope; no Mac production support inferred |
| S05 | [Avo pinned revision](https://github.com/nizwar/avo/tree/99153258763a9ccdf5d4c6de36bd631e4dd0dded) | Pinned candidate; subsequent local VMM tests passed, Android runtime remains unverified |
| S06 | [Avo architecture](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/docs/architecture.md) | Comparative process/display integration; design above is not a wholesale adoption |
| S07 | [Avo engine](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/docs/engine.md) | Device and graphics implementation notes; authors' metrics are not GlidePod metrics |
| S08 | [Avo GPU render code](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/engine/hvf/vmm/src/virtio/gpu/render.rs) | Verified presence of `transfer_read_iov` readback |
| S09 | [Avo HVF wrapper](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/engine/hvf/hvf/src/lib.rs) | Actual HVF/GIC calls, memory-map boundary; macOS 15 GIC comment |
| S10 | [Avo graphics build script](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/scripts/build-gfxstream.sh) | Source build path and candidate dependency pins |
| S11 | [Avo license](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/LICENSE) | Root Apache-2.0; engine Cargo manifests declare BSD-3-Clause; scope reconciliation required |
| S12 | [gfxstream](https://github.com/google/gfxstream), [CMake](https://github.com/google/gfxstream/blob/main/CMakeLists.txt) | API streaming and Apple build branches; not proof of a tested Cuttlefish Mac product |
| S13 | [MoltenVK runtime guide](https://github.com/KhronosGroup/MoltenVK/blob/main/Docs/MoltenVK_Runtime_UserGuide.md) | Metal backend, portability and interop extensions; exact runtime capability negotiation required |
| S14 | [GPUI manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml), [GPUI Apache license](https://github.com/zed-industries/zed/blob/main/crates/gpui/LICENSE-APACHE) | GPUI version observed: 0.2.2; Apache-2.0 |
| S15 | [GPUI macOS manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/Cargo.toml), [window implementation](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/src/window.rs) | Current `gpui_macos`/`gpui_apple` organization and native renderer integration |
| S16 | [Hypervisor](https://developer.apple.com/documentation/hypervisor), [entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.hypervisor) | CPU virtualization/process model and required entitlement |
| S17 | [Virtualization graphics](https://developer.apple.com/documentation/virtualization/graphics), [generic platform](https://developer.apple.com/documentation/virtualization/vzgenericplatformconfiguration) | Graphics and nested-VM APIs; no automatic Android Metal inference |
| S18 | [WWDC26: Expand the capabilities of your Virtualization app](https://developer.apple.com/videos/play/wwdc2026/224/) | macOS 27 custom Virtio device APIs; investigate as an alternative |
| S19 | [Metal IOSurface texture API](https://developer.apple.com/documentation/metal/mtldevice/maketexture(descriptor:iosurface:plane:)) | Candidate native sharing API; browser returned a JS documentation shell, so exact usage must be checked against the pinned SDK |
| S20 | [Android activity launch policy](https://source.android.com/docs/core/display/multi_display/activity-launch) | Display/task placement and security restrictions |
| S21 | [Build an Android widget host](https://developer.android.com/develop/ui/views/appwidgets/host) | Binding, configuration, sizes and widget-ID lifecycle |
| S22 | [WidgetKit](https://developer.apple.com/documentation/widgetkit/), [extension content](https://developer.apple.com/documentation/widgetkit/creating-a-widget-extension), [refresh](https://developer.apple.com/documentation/widgetkit/keeping-a-widget-up-to-date/) | SwiftUI/timeline architecture, supported interactions and budgeted refresh |
| S23 | [Apps in Spotlight](https://support.apple.com/guide/mac-help/open-apps-in-spotlight-mh35840/mac) | Current Mac app discovery; test older Launchpad separately |
| S24 | [AOSP licenses](https://source.android.com/docs/setup/contribute/licenses) | Apache preference and kernel GPL exception |
| S25 | [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) | License recommendation and retained obligations |
| S26 | [Android SDK terms](https://developer.android.com/studio/terms) | SDK binaries and open-source component distinction |
| S27 | [Play Integrity](https://developer.android.com/google/play/integrity/overview) | Certification/emulated-device limits |
| S28 | [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice), [notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) | Relevant distribution/service APIs; browser returned JS shells, implementation must verify against local SDK documentation |

## Evidence discipline

- Findings supported by source are distinct from proposed engineering choices and performance targets.
- The Avo repository's top-level feature list does not prove the absence of readback; the actual render code is the stronger evidence for presentation behavior.
- No upstream benchmark is restated as a GlidePod result. No Android image, VMM, GPUI program or game ran in this task.
- No third-party implementation source has been copied into this workspace. Prototype UI, sample apps and interaction code are original design fixtures.
- Some API details remain explicit G0 experiments: final-output IOSurface export, guest multi-display policy, generated launcher trust, and direct-distribution WidgetKit entitlements.

## Implementation follow-up

The 2026-10-23 implementation used the exact pinned Avo checkout, GPUI 0.2.2 and gpui-component 0.5.1 registry sources, the local Apple SDK Hypervisor headers, and [AOSP ResourceTypes.h](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/libs/androidfw/include/androidfw/ResourceTypes.h) (observed blob `e330410ed1a0890be266da0e62a3e6d1a25a3cdc`). Its measured evidence and license correction are recorded in [G0 foundation](../implementation/01-g0-foundation.md). Upstream test success is not Android boot evidence.
