# Graphics drivers, Metal translation and remaining costs

Date: 2026-11-08. This extends [the performance audit](22-performance-audit.md) with a source review, current upstream research and an isolated driver comparison. External VMM and gfxstream pins are unchanged.

Follow-up: [record 24](24-graphics-pressure-and-driver-settings.md) adds uncapped pressure measurements, a working-but-incompatible direct GLES Metal prototype and native driver settings. The measurements below remain the original refresh-limited baseline; the CLI-only selection and unimplemented-prototype descriptions record that earlier stage.

## What executes today

| Selection | GLES applications | Vulkan applications |
| --- | --- | --- |
| Automatic / Software | Guest ANGLE → guest SwiftShader Vulkan → CPU rasterization | Guest SwiftShader Vulkan → CPU rasterization |
| Metal, existing registration | Guest ANGLE → gfxstream Vulkan → host MoltenVK → Metal GPU | gfxstream Vulkan → host MoltenVK → Metal GPU |
| Metal, new optional registration | Guest ANGLE → gfxstream Vulkan → host KosmicKrisp → Metal GPU | gfxstream Vulkan → host KosmicKrisp → Metal GPU |

The VMM selects guest EGL `angle` in both modes. Software selects Vulkan `pastel`; acceleration selects `ranchu` and `virtio-gpu-asg`. The active user's Developer configuration inspected during this audit selects Metal. An emulator can use hardware API translation: emulating the Android device does not imply CPU rasterization. Host GPUI also uses the GPU, independently of the Android renderer selection.

The evaluated GLES renderer strings identify SwiftShader in Software and `Virtio-GPU GFXStream (Apple M5 Max)` in accelerated mode. The KosmicKrisp host probe identifies `DRIVER_ID_MESA_KOSMICKRISP`, Vulkan 1.4.363, driver info `vulkan-sdk-1.4.363 (git-269a955c89)`. Guest GLES reports ES 3.1. Reported versions are not a qualification of the complete guest stack.

## Current upstream changes affect the driver decision

The [MoltenVK repository](https://github.com/KhronosGroup/MoltenVK) now marks the implementation deprecated by Khronos and community-maintained, recommending KosmicKrisp for Apple Silicon. MoltenVK 1.4.2 remains its latest tagged release checked in this audit. The existing patched build remains useful as an independently qualified fallback for our targeted fixtures.

[LunarG](https://www.lunarg.com/products/kosmickrisp/) identifies KosmicKrisp as a Vulkan 1.4-conformant Mesa driver over Metal 4, including work sponsored for the Android Emulator. [Mesa's documentation](https://docs.mesa3d.org/drivers/kosmickrisp.html) requires Apple Silicon and macOS 26 or later. This makes it a concrete candidate on this M5 Max, not proof of superior performance for every application. It cannot replace the renderer on older macOS versions.

The pinned gfxstream already supports `ANDROID_EMU_VK_ICD=kosmickrisp`. Its MoltenVK branch forces single-queue semaphore behavior, disables Metal argument buffers and enables Metal heaps. Current gfxstream main `e4fae080c3d0effa313e3e036041426d178a90f3` retains those settings. They are compatibility decisions, not individually benchmarked optimal settings. Overriding them in a shell does not work because gfxstream sets them again; changing them requires a separately validated renderer build.

The existing Avo ASTC/memory-type adaptation layer is still present with either driver. These fixtures do not exhaustively test compressed formats or all advertised capabilities. A host driver's conformance cannot be carried through that adaptation and gfxstream merely by inference.

## Implemented candidate integration

- `register-gpu --driver kosmickrisp` pins the selected driver library, ICD manifest, gfxstream, Vulkan loader and source properties. Missing or changed artifacts fail verification. The ICD manifest must reference the registered library.
- Older registrations and the native registration picker retain MoltenVK. The runtime binds the process's ICD to the registration; ambient driver selection can no longer contradict its inventory.
- Runtime status and benchmark reports include `vulkan_driver`. Historical reports deserialize with an unknown driver (`null`); Software also has no host Vulkan driver.
- `tools/prepare-kosmickrisp-renderer.py` combines the locked SDK binary with the already evaluated gfxstream/loader in an immutable cache directory. It preserves input notices and records hashes/provenance. Cache reuse verifies all recorded files. The SDK is extracted locally with `copy_only=1`; no system driver is replaced.
- The external candidate is evaluation-only. It is not bundled, source-built by GlidePod, registered in the user's default store or promoted to Automatic.

Reproduction is in [runtime setup](../development/runtime.md#evaluate-kosmickrisp-on-apple-silicon). The [candidate lock](../../engine/kosmickrisp-renderer-lock.json) pins the SDK version, downloaded archive digest and actual driver digest.

## Comparison method

Local evidence is under `evidence/graphics-review-20261108/`. Both drivers use the same CLI, VMM binary, Android build, APKs, disposable guest disk, four vCPUs and manual 4 GiB RAM. Tests run sequentially with no concurrent builds. Each GLES matrix has six cases repeated twice, 180 measured frames after two seconds of warmup; every run repeats all 13 GLES correctness checks. Each Vulkan matrix has five cases repeated twice plus one 600-frame heavy run. A separate validation-enabled run is excluded from performance numbers.

The initial KosmicKrisp experiment used a process-local ICD override and a separately hashed candidate before typed driver registration was implemented. `candidate.json`, `measurement-context.json`, host `vulkaninfo`, renderer strings and engine logs preserve that identity. Its historical registration alone does not identify the experimental driver. Post-build checks separately exercise the final typed selection and packaged code.

VMM CPU covers the whole process during each invocation, including setup, correctness and warmup; 100% means one logical CPU core. Tests are refresh-limited, not maximum-throughput tests. Runs are blocked by driver, not randomized; cache/order effects and short-run variability remain. Vulkan intervals measure successful image acquisition and include deliberate swapchain recreation. They are not physical display intervals. Device timestamps are not calibrated against Metal GPU time.

### Measured results

All 24 GLES runs pass all 13 targeted checks, with zero measured intervals above 33.33 ms. All 22 Vulkan runs pass the fixture and include one acquisition interval above 33.33 ms in a workload that deliberately recreates its swapchain. Every matrix stays near 60 FPS. The separately validation-enabled KosmicKrisp run reports zero errors and zero warnings. GLES and Vulkan heavy-scene captured pixels were visually inspected; they are guest captures, not native window screenshots.

Values below are arithmetic means of the two repetitions' CPU averages and p95 values, not pooled percentiles:

| API / workload | MoltenVK VMM CPU | KosmicKrisp VMM CPU | MoltenVK interval p95 | KosmicKrisp interval p95 |
| --- | ---: | ---: | ---: | ---: |
| GLES, 16 objects, 720×1080 | 64.78% | 69.24% | 19.32 ms | 20.27 ms |
| GLES, 64 objects, 720×1080 | 60.28% | 58.82% | 19.74 ms | 19.77 ms |
| GLES, 16 objects, 1080×1920 | 59.58% | 62.30% | 19.40 ms | 19.32 ms |
| Vulkan, 16 objects, 720×1080 | 58.80% | 59.16% | 20.72 ms | 19.89 ms |
| Vulkan, 64 objects, 720×1080 | 58.97% | 61.34% | 21.43 ms | 19.65 ms |
| Vulkan, 16 objects, 1080×1920 | 59.59% | 61.92% | 21.58 ms | 19.72 ms |

The additional 600-frame Vulkan run gives MoltenVK/KosmicKrisp CPU 53.17%/54.08% and acquisition p95 20.95/19.48 ms. Its approximately ten measured seconds are insufficient for sustained thermal claims.

KosmicKrisp improves the measured Vulkan acquisition p95 in these cases, while using similar or slightly higher VMM CPU. GLES has no consistent advantage. The comparison does **not** establish a maximum-FPS, energy, physical-display-latency or general game-performance winner. The driver remains optional and the user's existing MoltenVK selection is preserved. The CPU values from record 22 should not substitute for this direct comparison: they came from a different run sequence and sample count.

## Direct GLES-to-Metal alternative

[ANGLE's support matrix](https://chromium.googlesource.com/angle/angle/+/main/README.md) lists the Metal backend as complete for GLES 2.0/3.0, while ES 3.1 is not listed as complete. A host-side GLES transport feeding macOS ANGLE Metal could remove the guest ANGLE-to-Vulkan/MoltenVK combination for GLES workloads. It is a viable architecture to prototype, not a flag for the current setup: guest Linux cannot load macOS Metal frameworks, and the current renderer deliberately enables Vulkan-only/surfaceless mode (`0x28`) with no host GLES context.

Such a prototype needs compatible guest GLES encoding, host EGL/ANGLE initialization, buffer/gralloc import, synchronization, composition, pixel/correctness checks and independent multi-display handling. Vulkan apps still need their own Vulkan-to-Metal driver. A shorter API chain by itself does not prove lower CPU time or better presentation latency, and losing currently exercised ES 3.1 behavior would be a compatibility regression.

For this codebase, qualifying KosmicKrisp and reducing common presentation overhead has a lower integration cost than replacing the GLES architecture. The direct ANGLE Metal path remains a separate experiment with its own acceptance criteria.

## Remaining optimization work

1. **Move readback off the render thread and avoid its intermediate copies.** Current `Renderer::flush` holds the scanout state lock while `transfer_read_iov` reads the image back. Avo's [off-thread readback change](https://github.com/nizwar/avo/commit/21bae7b7fdf7537eb191d09eb268d0292883e561) adds a gfxstream entry point and aligned host-memory import. Qualification must treat VMM, gfxstream patch and frame layout as one candidate, including resize/resource destruction/device loss and stale-generation tests.
2. **Replace file-backed frame/audio publication with owned shared memory.** [Upstream's change](https://github.com/nizwar/avo/commit/c94bd7cc1c11f6787b8fc8d8333bf2de02967ccf) avoids unnecessary mapped-file writeback. The current consumer accepts owned files with a 4 KiB pixel offset; the candidate announces `shm:<name>` and uses a 16 KiB offset. Add owner/epoch binding, crash cleanup and explicit ABI checks together. POSIX shared memory alone is not GPU zero-copy.
3. **Qualify IOSurface/native texture presentation.** Even after faster readback, the current Rust consumer copies pixels and GPUI uploads them again. GPU resource export, fence/lifetime handling, texture reuse and a GPUI import path are required to eliminate that round trip. Measure native window latency, dropped/repeated frames, CPU, memory and power in visible/minimized/resized multi-window states.
4. **Keep storage stalls separate.** Synchronous block flushes and the userdebug disk's scratch-space growth can affect interaction independently of graphics. The [async block I/O proposal](10-async-block-io-plan.md) still needs ordered durability and cancellation tests.

Neither MoltenVK nor the new candidate is established as the globally fastest solution. The measured driver comparison and explicit integration provide a basis for further decisions; native presentation, representative games, long-duration thermal/power behavior and whole-stack conformance remain separate evidence gates.

## Final implementation validation

- Locked workspace tests: 106 passed, 17 opt-in tests skipped by the ordinary run. Added checks cover legacy MoltenVK registrations, selected-driver hash changes and manifest redirection.
- Locked workspace Clippy with all targets and `-D warnings`, formatting, Python syntax and diff checks pass. Existing transitive future-Rust compatibility notices remain.
- The full development app/Android assets were rebuilt and the bundle passed deep/strict ad-hoc signature verification. No installed application or user runtime was replaced.
- The packaged CLI boots the immutable KosmicKrisp candidate through `--driver kosmickrisp`, even when its parent process specifies `ANDROID_EMU_VK_ICD=moltenvk`. Runtime status and benchmark JSON identify KosmicKrisp. A live process file inventory confirms the KosmicKrisp dylib is loaded and MoltenVK is absent.
- Post-build GLES 3 correctness, validation-enabled direct Vulkan (zero reported validation errors) and the separately executed real-guest independent display/input regression pass. These functional checks are not additional performance samples.
- The test VM is stopped, and the disposable store is returned to its original MoltenVK registration and Software selection. The user's existing configuration is unchanged. No commit, push or release was performed.

Evidence includes `comparison.json`, all 46 matrix runs, `typed-loaded-driver.txt`, `typed-runtime-status.json`, `typed-gles.json`, `typed-vulkan.json`, `typed-window-input.log`, `workspace-tests.log`, `clippy.log`, `package.log` and `validation.json` in the audit's ignored evidence directory.
