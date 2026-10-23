# Feasibility and technical choices

Date: 2026-10-23. All performance numbers elsewhere in this package are proposed acceptance targets unless explicitly described as a measurement of this prototype.

## Findings

| Finding | Evidence | Implication |
| --- | --- | --- |
| Official Cuttlefish setup uses Linux host tooling and KVM | [Cuttlefish setup](https://source.android.com/docs/devices/cuttlefish/get-started), [host utilities](https://github.com/google/android-cuttlefish) | A Mac product needs a different host runtime or a substantial host port. |
| Cuttlefish supports forwarding guest graphics through gfxstream | [GPU documentation](https://source.android.com/docs/devices/cuttlefish/gpu) | Reuse the guest graphics stack and protocol; a command-line flag alone is insufficient on macOS. |
| Avo contains a Rust HVF VMM and Cuttlefish GPU implementation | [pinned engine source](https://github.com/nizwar/avo/tree/99153258763a9ccdf5d4c6de36bd631e4dd0dded/engine/hvf) | A practical candidate exists, but its root Apache-2.0 license and engine BSD-3-Clause declarations need reconciliation before reuse. Attribution and a device/security audit remain required. |
| That implementation still reads frames from gfxstream | [pinned render.rs](https://github.com/nizwar/avo/blob/99153258763a9ccdf5d4c6de36bd631e4dd0dded/engine/hvf/vmm/src/virtio/gpu/render.rs) | GPU rendering and zero-copy presentation are separate claims. |
| GPUI and its current macOS crate declare Apache-2.0 | [GPUI manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml), [macOS manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui_macos/Cargo.toml) | Use the framework; audit selected dependencies rather than importing the whole Zed editor. |
| MoltenVK translates Vulkan onto Metal, with portability constraints | [runtime guide](https://github.com/KhronosGroup/MoltenVK/blob/main/Docs/MoltenVK_Runtime_UserGuide.md) | Determine supported features by negotiation and tests; a Vulkan version string is not a compatibility guarantee. |
| Android permits controlled multi-display activity placement | [launch policy](https://source.android.com/docs/core/display/multi_display/activity-launch) | Integration needs a privileged guest component, task monitoring and policy work, not screen cropping. |
| macOS 27 adds custom Virtio devices to Virtualization.framework | [WWDC26 session](https://developer.apple.com/videos/play/wwdc2026/224/) | This is a useful alternate backend investigation, not evidence of a ready-made Android gfxstream device. |

The Avo repository was created on October 17, 2026; the inspected revision is `99153258763a9ccdf5d4c6de36bd631e4dd0dded` dated October 22. Its authors' runtime claims have **not** been reproduced here. This review inspected the architecture/engine notes, VMM manifest, HVF wrapper, GPU readback implementation, graphics build script and license. The research phase did not audit the whole project or run it. The subsequent [G0 foundation work](../implementation/01-g0-foundation.md) compiled and ran its VMM tests (153 passed, 2 ignored), without booting Android.

## Backend decision

| Candidate | Advantages | Remaining work / cost | Decision |
| --- | --- | --- | --- |
| Rust VMM + Hypervisor.framework, Cuttlefish guest | Direct ARM64 execution; full control of devices and frame export; works without nested virtualization | Device correctness, memory safety, GPU ABI and maintenance belong to us | **Primary**; audit the candidate VMM and resolve its component license declarations before adaptation |
| QEMU + HVF + custom Cuttlefish devices/gfxstream | Mature generic virtualization; useful diagnostic comparison | Android boot wiring and accelerated transport still need integration; GPL distribution obligations | Contingency/reference backend, as a separate executable |
| Virtualization.framework + custom Virtio (macOS 27) | Apple manages more of the virtual machine | Raises OS floor; custom device/guest integration and graphics export need proof | Time-box a second backend experiment after the primary gate |
| Linux VM + stock Cuttlefish/KVM | Keeps official Cuttlefish host behavior | Nested virtualization availability varies; two kernels; GPU translation remains unresolved; larger resource use | CI/reference environment, not the desktop architecture |
| Android Emulator / Goldfish host | Existing Mac graphics engineering | Different guest/device assumptions; binary licensing and integration burden; does not preserve the requested Cuttlefish base | Source-level reference and comparison only |
| Full software emulation/rendering | Broad diagnostic fallback | Unacceptable primary gaming performance | Recovery and test mode only |

Hypervisor.framework accelerates virtual CPUs. It does not supply an Android GPU driver or physical GPU passthrough. Virtualization.framework's Linux display device likewise does not establish that Android Vulkan commands can execute on Metal. [Apple Hypervisor](https://developer.apple.com/documentation/hypervisor), [Apple VM graphics](https://developer.apple.com/documentation/virtualization/graphics).

## Product envelope

Initial qualification target: Apple Silicon M1 or later, macOS 15+, 8 GB system memory minimum with a conservative 2 GB guest preset; recommend 16 GB and a 4 GB guest. Default to 4 vCPUs where appropriate, capped against actual host resources. Reserve headroom for macOS; do not use fixed percentages without pressure monitoring. Propose 20 GB free space for installation and updates, calculate exact requirements from the selected image manifest. These are sizing proposals, not tested minimum specifications.

Use an ARM64-only Cuttlefish product. Java/Kotlin-only applications and `arm64-v8a` native applications are candidates. ARM32-only, x86-only and x86_64-only APKs are outside v0.1; Rosetta for Linux is not a drop-in Android native bridge. Guest kernel page size and Mac mapping granules must be handled separately. Choose one reproducible AOSP Android 16 or 17 branch only after boot/GPU qualification; do not silently follow CI's latest image. Record the build fingerprint, manifest revision and hashes.

The compatibility promise is a published, versioned test matrix. It excludes claims of universal game support, Google Play certification, DRM playback, banking compatibility, anti-cheat acceptance, or Play Integrity success. The [Play Integrity API](https://developer.android.com/google/play/integrity/overview) explicitly distinguishes certified environments. A root-off VM is still a virtual device.

## Decisions that can change after G0

1. **VMM reuse:** accept the audited VMM fork if boot/device tests pass; otherwise compare a scoped QEMU/HVF proof before budgeting a new VMM.
2. **Presentation:** shipping may start with bounded readback only if battery, latency and bandwidth gates pass. Zero-copy remains a separate improvement with its own evidence.
3. **Display topology:** begin with one physical virtual scanout per task and a hidden control display. If limits or hotplug behavior fail, investigate a privileged guest virtual-display compositor path without changing the host task protocol.
4. **Guest customization:** start with a guest system service and small compositor/device changes. Escalate to a framework fork only for a reproduced task-routing or resize failure, with regression tests and an upstream patch strategy.
5. **Native launcher trust:** generated local bundles must pass a fresh-user Gatekeeper/LaunchServices experiment. The fallback is a notarized generic launcher with reduced per-app identity, clearly reported as an incomplete integration feature.
