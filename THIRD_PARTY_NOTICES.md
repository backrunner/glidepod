# Third-party notices

Original GlidePod source, documentation and the layered package icon are licensed under Apache-2.0. The host foundation uses GPUI 0.2.2 and gpui-component 0.5.1, both declaring Apache-2.0, plus the exact Rust dependency graph in `Cargo.lock`.

`tools/license-inventory.py` records the target-resolved graph and copies available upstream license/notice texts into `THIRD_PARTY_LICENSES.txt`. The public [JSON inventory](licenses/dependency-inventory.json) records each package's declared SPDX expression and available notice files; the tool also writes a local copy under `evidence/g0/`. It includes build and development dependencies, not only statically linked code. Packages without bundled top-level notice text require upstream-source follow-up before distributing a binary containing them. Publishing this source repository does not establish clearance for a bundled runtime or binary release.

The source tree includes an Apache-2.0 patch against MoltenVK 1.4.2. Context from the upstream files retains its original ownership. See [patch attribution](engine/patches/MoltenVK-NOTICE.txt), the [upstream license](engine/patches/MoltenVK-LICENSE.txt) and the [description of modifications](engine/patches/README.md). The patch is not an upstream-endorsed change.

Apple frameworks are provided by macOS and are not redistributed here. Metal and Hypervisor calls in GlidePod are original adapters against the local SDK.

The original direct Vulkan diagnostic APK uses Android NDK r30 (30.0.16248370) and statically links its C++ runtime. The APK retains the NDK LLVM/runtime and Android sysroot notice collections under `assets/licenses/`. The local development APK optionally includes the pinned Khronos Vulkan Validation Layers 1.4.363.0 arm64 binary, enabled only by an explicit diagnostic request. Its Apache-2.0 and permissive exception notices are retained in `android/vulkan/licenses/Khronos-Validation-Layer.txt` and inside the APK. NDK compiler tools are not bundled. The Vulkan diagnostic and SPIR-V shader sources are original Apache-2.0 project code; validation passes are not Khronos certification.

The candidate Avo VMM is evaluated separately in an ignored checkout. No Avo source or binary is linked into or copied into the GlidePod app. Its repository root declares Apache-2.0, while its `engine/hvf` workspace and crates declare BSD-3-Clause. Resolve that component scope and obtain the appropriate copyright/license text before reuse. The pinned upstream also references gfxstream, ANGLE and MoltenVK; none is bundled by this foundation.

No AOSP images, Linux kernel, Google Mobile Services, user APKs, or proprietary Android compatibility layers are distributed. The metadata fixture generator creates original synthetic test data with no executable application content.
