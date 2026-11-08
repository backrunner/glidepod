# Graphics pressure, direct GLES Metal and driver settings

Date: 2026-11-08. Follow-up to [record 23](23-graphics-drivers-and-metal.md). The subsequent source-built candidate and correctness fixes are tracked in [record 25](25-direct-gles-metal-correctness.md); results below retain the original prebuilt probe.

## Questions and implementation

GLES-to-Vulkan translation is API/shader translation, not lossy image compression. It can add CPU work, compilation, synchronization and compatibility workarounds. Floating-point behavior and implementation defects can change pixels; correctness must be tested separately from throughput. Fewer API layers are not sufficient evidence of a faster or correct emulator.

The normal accelerated GLES path remains guest ANGLE → gfxstream Vulkan → host Vulkan driver → Metal. Vulkan applications skip guest ANGLE. MoltenVK is the existing registration; the optional KosmicKrisp candidate uses Metal 4 on Apple Silicon/macOS 26+. The product does not silently switch either driver or promote the experimental direct GLES path.

**Graphics → Vulkan driver** now shows the live runtime driver and next-start selection independently. Each driver has registration and selection actions. A version 2 atomic registry retains both immutable artifact inventories and the selected driver; old flat MoltenVK registrations migrate without a selection change. Switching requires Android to be stopped and verifies hashes and the ICD manifest before changing selection. CLI equivalents are `gpu-driver`, `gpu-driver moltenvk` and `gpu-driver kosmickrisp`. Libraries remain explicitly registered external components.

## Reproducible pressure workloads

`tools/evaluate-gles-stress.py` drives the running disposable VM, retaining a fresh report per case and rejecting mismatched run identities, durations, invalid pixels and correctness failures. It supports GLES and direct Vulkan. Each case has two seconds of warmup, 30 measured seconds and two repetitions. The second pass reverses workload order. The two driver matrices use the same normal VMM, Android disk/build, four vCPUs, manual 4 GiB RAM and workload APKs. Runs are sequential, with no concurrent builds or other benchmark processes. Drivers are tested in blocks rather than randomized; cache/order/thermal effects remain possible.

| Workload | Target | Work per completed frame |
| --- | --- | --- |
| GLES draw calls | 512×512 RGBA8 | 4,096 scissored draws, deterministic uniforms |
| GLES fragment | 2048×2048 RGBA8 | 8 blended full-screen draws, 128 dependent ALU iterations |
| GLES texture | 2048×2048 RGBA8 | 8 blended full-screen draws, 32 dependent texture samples |
| Vulkan indexed 256 | 512×512 RGBA8 + depth | 256 textured meshes, 3,072 indices per mesh |
| Vulkan indexed 64 | 2048×2048 RGBA8 + depth | 64 textured meshes, 3,072 indices per mesh |
| Vulkan indexed 4096 | 2048×2048 RGBA8 + depth | 4,096 textured meshes, 12,582,912 indices per frame |

GLES calls `glFinish` each frame; Vulkan records, submits and waits for a fence each frame. Neither timed loop swaps, acquires a presentation image, reads pixels back or presents to a Mac window. Shader/pipeline setup, feature checks, readback and display teardown are outside the timed loop. This deliberately measures completion latency and throughput with one frame in flight; it is not maximum pipelined queue throughput or game FPS. The VM compositor and guest scheduling remain part of the environment.

The GLES workload is shared C++ source (`android/benchmark/native/stress.hpp`) with an optional macOS ANGLE reference wrapper. It checks a known RGBA8 pixel and nonempty final image, records a full-image hash and 49 RGBA probes, and repeats the existing GLES feature checks. Direct Vulkan repeats compute, texture/depth, synchronization and dynamic-rendering checks before its pressure loop. Separate Vulkan runs enable synchronization validation; performance runs disable validation.

VMM CPU covers setup, correctness, warmup and teardown as well as the pressure loop. 100% means one logical core. The runner converts XNU Mach CPU ticks using `mach_timebase_info`. The first direct-Metal collection used an incorrect tick-as-nanosecond CPU conversion; its summary is corrected by the measured 125/3 ratio, with the original summary retained. Rendering timings were unaffected.

Evidence lives under `evidence/graphics-stress-20261108/`; `context.json` records artifact hashes and host identity. Results are targeted local fixtures, not CTS, native-window smoothness, energy measurements or representative game qualification.

## Direct GLES → Metal prototype

This was implemented and booted, not inferred from a host-only demo. A separate pinned Avo checkout applies the existing memory/host-service patches plus `engine/patches/avo-direct-gles-metal-probe.patch`: enable host GLES/EGL/Vulkan/surfaceless flags (`0x39`), select the guest `emulation` EGL transport, and advertise ES 3.0. The normal product VMM is not replaced.

The host uses ANGLE from the official Android Emulator 37.2.13 Apple Silicon archive, with archive and dylib hashes recorded in `engine/direct-gles-metal-probe-lock.json`. Libraries are local evaluation assets, not redistributed in the app. Launch environment:

```sh
DYLD_LIBRARY_PATH=/absolute/path/to/emulator/lib64/gles_angle
ANGLE_DEFAULT_PLATFORM=metal
ANDROID_EGL_ON_EGL=1
```

The guest reports `Android Emulator OpenGL ES Translator (ANGLE (Apple, ANGLE Metal Renderer: Apple M5 Max, Version 27.2 (Build 26B123)))`, ES 3.0, ANGLE revision `fbf66f49c7cc`. This confirms guest GLES remoting reaches host ANGLE Metal. The Vulkan driver field still identifies MoltenVK because Vulkan applications retain their separate Vulkan path; it is not the GLES backend label.

The candidate fails two of eleven available GLES 3.0 feature checks:

- Indexed triangle fan: pixel (32,16) should be blue but is red.
- Indexed primitive restart: pixel (48,48) should be yellow but is black.

The normal path also exercises two ES 3.1 indirect-draw checks that this candidate cannot expose. The prototype is therefore incompatible and cannot be promoted, regardless of diagnostic throughput. Reports preserve failing pixels and return failure; the runner's explicit incompatible-probe option continues diagnostics but still exits with failure. The independent pressure scene passes its own GL-error/reference/readback checks, which does not repair the failed GLES features.

The native host-only ANGLE smoke is functional evidence only: it excludes the VM, used a different context and overlapped boot activity. It is excluded from comparative performance numbers. The same supplied ANGLE build could not initialize its Vulkan backend; no silent backend substitution was used. The actual guest prototype and normal guest path also use different ANGLE implementations/revisions, so comparisons describe complete candidate stacks rather than isolating the cost of exactly one translation layer.

## Measured results and verification

All 24 normal-driver pressure runs pass. Every GLES run passes all 13 feature checks; every Vulkan run passes its nine checks, including pressure completion and readback. Separate synchronization-validation runs for each driver report zero errors and zero warnings. The six direct-Metal runs fail the full GLES acceptance gate and remain diagnostic evidence only.

Values below are arithmetic means of the two repetitions' statistics, not pooled percentiles. Completion time is milliseconds; lower is better. These are not presented/displayed frame times.

| Workload | MoltenVK p95 | KosmicKrisp p95 | MoltenVK completed frames/s | KosmicKrisp completed frames/s | MoltenVK / KosmicKrisp VMM CPU |
| --- | ---: | ---: | ---: | ---: | ---: |
| GLES draw calls | 3.130 | 4.519 | 350.9 | 245.8 | 151.78% / 180.86% |
| GLES fragment | 5.323 | 5.260 | 197.2 | 199.7 | 119.96% / 125.06% |
| GLES texture | 5.175 | 4.990 | 204.2 | 212.7 | 121.91% / 124.10% |
| Vulkan indexed 256 | 0.742 | 0.755 | 2098.2 | 1973.8 | 152.27% / 156.74% |
| Vulkan indexed 64 | 0.840 | 0.883 | 2314.9 | 2197.3 | 147.68% / 152.90% |
| Vulkan indexed 4096 | 2.319 | 3.686 | 471.9 | 292.8 | 146.60% / 167.57% |

MoltenVK is about 43% faster in GLES draw-call throughput and 61% faster in the heaviest Vulkan case. KosmicKrisp has about 4% higher texture-workload throughput; the small fragment difference and variable light Vulkan cases do not establish a decisive advantage. These results support retaining the existing MoltenVK selection and exposing KosmicKrisp as an explicit alternative. They do not establish a universally fastest driver. In particular, record 23's FIFO acquisition timings answer a different question than this offscreen fence-completion test.

| Incompatible direct GLES Metal prototype | Completion p95 | Completion p99 | Completed frames/s | VMM CPU |
| --- | ---: | ---: | ---: | ---: |
| Draw calls | 2.356 | 4.450 | 534.3 | 87.77% |
| Fragment | 9.683 | 42.704 | 103.2 | 33.18% |
| Texture | 5.251 | 38.652 | 183.4 | 36.59% |

The prototype improves draw-call throughput but the existing MoltenVK path has about 91% higher fragment-workload throughput. The prototype also has long fragment/texture completion tails. The source of those tails was not isolated by a trace. These observations, together with the correctness failures, rule out recommending this prototype as a general upgrade. Lower VMM CPU does not establish lower total system energy.

Each case's final-image hash is stable across both repetitions on the same stack. All three GLES images have identical full hashes across MoltenVK and KosmicKrisp. Native Vulkan images differ in full hashes between drivers; direct Metal fragment/texture images also differ from MoltenVK. All 49 sampled RGBA probes agree in every compared case. Probe agreement does not establish complete pixel equality or quantify the differences elsewhere in those images. The separate explicit pixel checks remain necessary.

Verification and delivery:

- Locked workspace tests: **107 passed**, 17 opt-in tests skipped. Registration tests cover legacy migration, retaining two entries, reopening the store, corrupted artifacts and preserving the selected driver when a switch fails.
- Workspace Clippy with all targets and `-D warnings`, Rust formatting, Android native/Java build, Python syntax and diff checks pass. The development bundle was rebuilt and passes deep/strict ad-hoc signature verification. Its executable build UUIDs match the tested build and its workload APK hashes match exactly; ad-hoc signing changes executable file bytes. The packaged driver-catalog command also succeeds.
- A live CLI switch is rejected without changing the selected driver. A stopped switch persists, and the restarted VM's status and loaded-library inventory identify KosmicKrisp with no MoltenVK library loaded. Both drivers are exercised through the final registry implementation.
- The isolated native settings app was launched, but CUA returns `cgWindowNotFound` for both the app path and bundle ID. Inventory lists GlidePod running without accessible windows. Native clicks, screenshots and minimum-window visual acceptance are **not verified**; no substitute screenshot is claimed.
- The disposable VM is stopped, the original normal VMM is restored, and its selection is returned to MoltenVK with Software configured. The user's existing Metal/MoltenVK selection is preserved. No commit, push, release or global app installation occurred.

`comparison.json`, each matrix's `summary.json` and per-run JSON, driver inventories, validation reports, build/test logs and `validation.json` retain the evidence. `run-remaining.py` records the driver-switch sequence used locally. The extra 4096-object MoltenVK cases were added after its first two Vulkan cases; the default runner now includes all three Vulkan cases.

## Remaining work

The direct GLES candidate first needs indexed-fan and primitive-restart fixes, explicit ES 3.1 feature planning, buffer import/lifetime tests and multi-display correctness. Production presentation still performs GPU readback, CPU copying and host texture upload; these costs are deliberately excluded from the offscreen pressure loop. They remain a shared optimization opportunity independent of which API driver wins a microbenchmark. Readback changes and IOSurface integration need their own end-to-end correctness and native presentation measurements.

Sources: [ANGLE backend matrix](https://chromium.googlesource.com/angle/angle/+/main/README.md), [Mesa KosmicKrisp requirements](https://docs.mesa3d.org/drivers/kosmickrisp.html), [official Android Emulator package index](https://dl.google.com/android/repository/repository2-3.xml).
