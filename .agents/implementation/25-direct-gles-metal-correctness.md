# Direct GLES Metal: source build, correctness and pressure

Date: 2026-11-08. Follow-up to [record 24](24-graphics-pressure-and-driver-settings.md). The subsequent long-tail investigation and bounded readback patch are tracked in [record 26](26-metal-readback-tail.md); measurements here retain this earlier candidate.

## Scope and result

Continue the direct guest GLES → gfxstream GLES transport → host ANGLE Metal experiment. Vulkan applications keep their separately selected Vulkan driver. This work does not change the normal VMM, Automatic renderer policy or user driver selection. The candidate advertises GLES 3.0; passing targeted tests does not establish GLES conformance, ES 3.1 support or representative game compatibility.

## Isolating the failures

A standalone host EGL reproducer bypasses Android and gfxstream. It reproduces both the indexed-fan and primitive-restart failures with the original Android Emulator ANGLE libraries. The same shared C++ regression matrix runs through JNI in the Android benchmark, so the host and guest expectations cannot drift independently.

The matrix contains 216 cases: six fan/strip/restart patterns, three index widths (8/16/32 bits), three draw entry points (elements, instanced elements, range elements), two index-buffer offsets and back-face culling on/off. It checks exact flat-shaded reference colors with a two-value channel tolerance and GL errors. Leading, trailing and consecutive restart indices and repeated subdata updates exercise per-range index offsets and conversion-cache invalidation.

| Host ANGLE Metal build | Cases | Failures |
| --- | ---: | ---: |
| Original Emulator build, `fbf66f49c7cc` | 216 | 168 |
| Pinned upstream `4d61277d5827d69d01934ef9930e935a34d07a19` | 216 | 108 |
| Pinned upstream plus GlidePod fan patch | 216 | 0 |
| Same patched artifact, Metal API Validation enabled | 216 | 0 |

The current upstream source resolves the restart cases. Indexed flat triangle fans still take a fast path that bypasses provoking-vertex index rewriting. Routing them through the normal rewriter is necessary but insufficient: the indexed Metal rewrite shader also needs fan support. The completed patch adds that shader case, selects triangle-list output, and rotates each triangle so Metal receives the GLES provoking vertex first while preserving winding. Each primitive-restart range retains its own fan anchor. Both generated shader-source files are included in the patch.

The two original failing Java checks remain unchanged. The additional matrix is a mandatory correctness gate, not a replacement that weakens them. The original MoltenVK guest path also passes all 216 cases and its existing ES 3.0/3.1 checks.

## Cross-context command submission

Integrating the updated source exposed a second defect that a single-context renderer cannot detect. The VM stalled inside `Texture::getBytes` → `CommandQueue::ensureResourceReadyForCPU`, and Android exceeded its 120-second boot deadline. A standalone two-thread regression also reproduced incorrect pixels: context B completed a newer command buffer while context A still had an older unsubmitted buffer. The global maximum-completed serial incorrectly treated A as complete. Waiting through the queue from its front could instead wait forever on A's unrelated, unsubmitted buffer.

`angle-metal-command-serials.patch` records exact outstanding command buffers and their committed/scheduled state. Readback waits for its resource's command buffer, completion removes only that serial, and the fast completed-serial value advances only over a contiguous completed prefix. Scheduled callbacks execute outside the queue lock. This preserves pending work in other contexts without serializing every draw or adding an unconditional GPU finish.

`tools/gles-contexts.cpp` runs 32 iterations with independent contexts and another 32 with shared contexts. B must finish its own readback while A remains unflushed; A must subsequently return the correct pixels. The shared-context variant also exchanges texture ownership with explicit GL fences, waits and pixel checks. Before the patch, the standalone test read transparent black instead of A's opaque red. After the patch, both variants pass with Metal API Validation enabled, and the VM completes boot, the full GLES correctness matrix and clean shutdown.

## Vulkan presentation in the mixed backend

Vulkan's guest-local checks and synchronization validation initially passed, but the independently captured host display was black. The failure was retained instead of accepting guest success as presentation success. Temporary gfxstream tracing showed queue submissions without associated released ColorBuffers; source inspection found the missing association in the externally backed Android native-image creation path.

The native-image creation path binds the memory internally and bypasses guest `vkBindImageMemory` tracking. `gfxstream-metal-native-image.patch` preserves the ColorBuffer association for those externally backed images. Existing render-pass/layout tracking then updates the GL backing before GLES composition. The fixed path passes host colored-scene capture, swapchain recreation and Vulkan synchronization validation (zero errors and warnings). It uses the existing synchronized copy fallback when GL/Vulkan memory is not shared; this is not zero-copy interop.

The first simultaneous-display attempt also exposed a harness startup race: the Vulkan result provider creates its process before the activity has drawn. The runner now waits for the correct activity's drawn, visible window before opening the second display. Both the manual run and the updated automated runner pass. The final automated run captures 689 Vulkan frames and 346 GLES frames on different displays in the same VM epoch, with 18.68 seconds of overlapping invocations. These are functional checks, not comparative throughput measurements.

## Reproducible local builds

`engine/angle-metal-lock.json` pins the ANGLE, depot_tools and gfxstream revisions, complete patch hashes and GN/CMake arguments. `tools/build-direct-gles-metal.py` verifies the exact source patch, syncs pinned dependencies, builds Metal-only ANGLE libraries and the patched gfxstream backend and publishes an immutable content-derived artifact directory. It retains library and notice hashes, dependency-manifest hash and Xcode identity. Modified checkouts or mismatched cached artifacts are rejected rather than reset. The second build reused the same libraries with Ninja reporting no work.

```sh
rtk proxy python3 tools/build-direct-gles-metal.py
rtk proxy python3 tools/build-memory-engine.py --host-services --gles-metal
rtk proxy python3 tools/check-direct-gles-metal.py \
  --artifact /absolute/path/printed/by/the/builder \
  --output evidence/direct-metal-host-new-run
```

Use the artifact directory printed by the builder for a new build, and a fresh evidence directory for each check. The host runner verifies the library hashes and current lock, compiles both regressions with warnings as errors, and enables Metal API Validation for topology and both context variants. Run it separately from performance measurement.

These commands build and check local evaluation components; they do not register them or change settings. The VMM builder accepts `--data-dir` to explicitly register its output against a stopped disposable runtime. The GLES experiment requires the separately built ANGLE artifact at launch:

```sh
rtk proxy env DYLD_LIBRARY_PATH=/absolute/path/from/angle/build/output \
  ANGLE_DEFAULT_PLATFORM=metal ANDROID_EGL_ON_EGL=1 \
  target/debug/glidepodctl --data-dir .cache/your-disposable-store --json start
```

Select Metal and retain a registered Vulkan driver in that same store before starting. The runtime Vulkan-driver field continues to identify the driver for Vulkan applications. Inspect the guest GL renderer/version to identify the independent GLES backend.

The historical `engine/direct-gles-metal-probe-lock.json` still describes the incompatible prebuilt Emulator probe; it is not repurposed to represent these source-built libraries. Local caches and notices are not a claim of distribution clearance or an App Store qualification.

## Validation evidence

Evidence is retained under `evidence/direct-metal-20261108/`:

- `release-topology.json`, `release-contexts.json` and `release-contexts-shared.json`: 216 topology cases and 32 iterations of each context variant pass with Metal API Validation enabled.
- `control-gles.json`: the normal MoltenVK path passes 14 GLES 3.0/3.1 checks, including the shared 216-case matrix. The direct candidate passes its 12 GLES 3.0 checks, including the same matrix, in `mixed-displays/gles.json`.
- `final-vulkan.json`: the final immutable stack passes all 10 Vulkan checks, forced swapchain recreation and synchronization validation with zero errors and warnings; independent host capture contains the colored scene and 159 frames.
- `mixed-displays/report.json`: simultaneous GLES/Vulkan displays pass on the final stack.
- `input-final/report.json`: all 12 independent-display/input checks pass, including held input, focus reset, watchdog release, controller axes and multi-touch. Captured displays advance 36 and 35 frames over two seconds; synthetic motion acknowledgement p95 is 353 microseconds. This does not measure physical input or native macOS window presentation latency.
- `complete-stack-build.log` and `complete-stack-recheck.log`: source build and a no-work rebuild produce the same immutable directory. `candidate-provenance.json` records the actual loaded libraries in the running VMM.
- `host-regression/summary.json`: the reusable host runner independently verifies the final immutable artifact, builds both test programs with `-Werror`, and passes all three Metal-validation runs.

The complete candidate artifact is `.cache/direct-gles-metal/e78d8f4ca61277f2`. Its SHA-256 identities are:

| Component | SHA-256 |
| --- | --- |
| `libEGL.dylib` | `dae92754967374a8c5ac760aae89817c9eacc32255d64748e51947a61897a198` |
| `libGLESv2.dylib` | `911f26e22ee1a6d9f35a8acdbe39dd906085371dff4497e5e63522635de10548` |
| `libgfxstream_backend.dylib` | `92b56a89f208d84b75d42b9ea58446b5cf8946d57e9d486fb51987d05bdfa274` |
| GLES experiment VMM | `522eb3de3b6946b2a36ee6571d308fc56e0a782e8e2ba607dd7d0bed84a42c01` |

The independent-context and topology host checks use these same ANGLE library hashes. The source artifact retains 832 library/notice files with individual hashes. The Android benchmark APK was rebuilt and copied into the development app; packaged APK hashes match the tested build outputs, and the development bundle passes deep/strict ad-hoc signature verification.

The first manual-artifact attempt returned a transport EAGAIN error and hit the shutdown timeout. The first reproducible fan-only build then hit the boot deadline. Those diagnostics and the blocked host stack are retained; neither counts as successful guest validation. The command-serial patch addresses the separately reproduced synchronization defect. Final acceptance uses both ANGLE patches, the gfxstream native-image patch and the reproducible VMM.

## Controlled pressure methodology

`tools/evaluate-gles-stress.py` compares the complete source-built direct stack with the original normal VMM and guest ANGLE → MoltenVK path. Both use the same Android disk, rebuilt APK, four vCPUs, manual 4 GiB RAM, Apple M5 Max and macOS 27.2. Actual loaded libraries are recorded for both VMM processes. Each stack runs draw-call, fragment and texture workloads twice, reversing workload order on the second pass, with two seconds of warmup and 30 seconds measured per run. No compiler, host regression or other benchmark runs concurrently.

The workloads and single-frame `glFinish` completion scope are unchanged from record 24. Every run must pass the complete GLES correctness gate, including the 216-case topology matrix. CPU measurements include correctness/setup/warmup/teardown, and 100% denotes one logical core. These CPU values do not measure system energy. Values reported for a workload are arithmetic means of two runs' statistics, not pooled percentiles. Stacks run in blocks and differ in ANGLE revision, transport and gfxstream build; thermal/cache/order effects remain possible. Results do not isolate the cost of one translation layer.

All 12 runs pass. `patched-pressure/`, `moltenvk-pressure/` and `comparison.json` retain per-run reports and the comparison. All three final images have stable full-image hashes across both repetitions and identical hashes across these two stacks; their 49 RGBA probes also agree. This is stronger fixture evidence than the original incompatible prebuilt probe, but not general image equivalence for arbitrary games.

| Workload | MoltenVK p95 / p99 (ms) | Direct p95 / p99 (ms) | MoltenVK / direct completed frames/s | MoltenVK / direct VMM CPU |
| --- | ---: | ---: | ---: | ---: |
| Draw calls | 3.879 / 4.320 | 2.270 / 3.975 | 316.5 / 547.7 | 165.20% / 89.33% |
| Fragment | 5.164 / 5.356 | 5.199 / 37.840 | 208.8 / 186.2 | 121.42% / 35.49% |
| Texture | 5.023 / 5.278 | 4.898 / 37.170 | 215.2 / 200.4 | 120.77% / 35.87% |

The source-built candidate improves draw-call throughput by 73.1% in these runs, while fragment and texture throughput are 10.8% and 6.9% lower. Its much lower VMM CPU is useful evidence of different host work/wait behavior, not an energy result. Heavy-workload p99 completion remains approximately 37–38 ms versus about 5.3 ms on the normal path. The candidate therefore remains an explicit source experiment; these results do not justify changing the default backend. The Vulkan-driver selection remains independent, and record 24's MoltenVK/KosmicKrisp comparison is not replaced by this GLES experiment.

After stopping the VM, `tools/gles-stress.cpp` runs the same shared C++ workload directly against the exact candidate ANGLE libraries, with Metal validation disabled and no concurrent build. These additional 30-second runs are single-pass diagnostic controls, not a second VM comparison:

| Host-only control | Completion p95 | Completion p99 | Maximum completion |
| --- | ---: | ---: | ---: |
| Fragment | 4.923 ms | 5.003 ms | 6.812 ms |
| Texture | 4.658 ms | 4.775 ms | 5.192 ms |

Both pass the workload pixel/error checks. `host-pressure/` retains their outputs. The approximately 37 ms tail is absent in this host-only control, so it is not reproduced by the same ANGLE shaders/backend alone. This narrows the next investigation to the Android/gfxstream/VM composition and synchronization environment; it does not identify a specific wait or prove a causal scheduler defect. Trace guest submission, virtio/gfxstream processing and host completion together before changing synchronization.

## Delivery state

The disposable VM stopped cleanly after both candidate and baseline runs. Its original engine registration is restored, MoltenVK remains selected, both Vulkan-driver registrations are retained, and its renderer preference is returned to Software. The user's separate runtime registration, Metal preference and MoltenVK registration remain byte-identical. `restored-runtime.json` and `validation.json` record this state. The local development app is repackaged with the updated benchmark; source and runtime experiment changes remain local.

## Remaining boundaries

No ES 3.1 features are advertised by this experiment. External-buffer import/lifetime, complete CTS, representative games and end-to-end native presentation remain separate gates. The production display transport still includes readback/copy/upload. Offscreen completion timings cannot quantify that cost or establish game FPS or whole-system energy use.

Source references: [pinned ANGLE context](https://chromium.googlesource.com/angle/angle/+/4d61277d5827d69d01934ef9930e935a34d07a19/src/libANGLE/renderer/metal/ContextMtl.mm), [pinned index-rewrite shader](https://chromium.googlesource.com/angle/angle/+/4d61277d5827d69d01934ef9930e935a34d07a19/src/libANGLE/renderer/metal/shaders/rewrite_indices.metal).
