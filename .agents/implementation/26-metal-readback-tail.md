# Direct GLES Metal: bounded readback and completion tails

Date: 2026-11-08. Follow-up to [record 25](25-direct-gles-metal-correctness.md).

## Investigation

The prior direct GLES stack had approximately 37–38 ms p99 completion in heavy offscreen workloads, while a host-only control using the same ANGLE libraries did not reproduce that tail. This investigation keeps the existing Vulkan driver selection and targets the shared VM graphics path.

Host sampling shows `gpu-render` spending time inside display `transferReadIov` → `ColorBufferGl::readPixels` → ANGLE `ReadTexturePerSliceBytes` → Metal `getBytes`, including CPU texture decompression. The same VMM thread handles guest GLES pipe transfers, so synchronous display readback delays unrelated command/reply work. The pinned ANGLE fallback calls Metal once for every pixel row, repeating tiled/compressed texture access.

Temporary slow-call timers in gfxstream distinguish color-buffer reads, pipe reads and host `glFinish`. In the final 30 seconds of the diagnostic log, 140 color-buffer reads exceed 10 ms (median 18.90 ms, maximum 28.75 ms); there are no host `glFinish` calls above that threshold. Consecutive primary/secondary display reads account for the approximately 30–40 ms transport stalls. These timers and the host sampler are diagnostic evidence, excluded from final performance measurement. Their patch and logs are retained; instrumentation is removed from the final backend.

Enabling ANGLE's existing `copyTextureToBufferForReadOptimization` is a useful negative control, not the accepted fix. In the 30-second fragment diagnostic it reduces p99 from 38.97 to 14.77 ms but lowers completion throughput from 186.0 to 131.8 frames/s. The final candidate does not force this feature.

## Change

`engine/patches/angle-metal-readback-strips.patch` reads bounded strips in ANGLE's existing CPU fallback. A strip uses at most 1 MiB of temporary storage, or one source row if a row itself exceeds that budget. Checked arithmetic guards allocation size. Successive rows reuse the strip; reverse-order packing loads strips in reverse order. The final short strip passes its exact byte span to Metal. Format conversion, destination stride, skipped pixels/rows, mip and layer selection, synchronization and GPU-error checks retain their existing paths. PBO readback and the existing driver-specific GPU-buffer optimization are unchanged.

The first strip implementation incorrectly passed a full-sized span for a partial final strip. The new large-image regression triggered ANGLE's release assertion, and the VM also rejected that candidate during boot. The corrected implementation supplies exactly `row_pitch × strip_height`; failed artifacts and logs are retained, not accepted as successful validation.

The source lock adds this third ANGLE patch alongside the prior topology and command-serial fixes. The immutable accepted-build candidate is `.cache/direct-gles-metal/3a6dad2877b0a1e5`; its exact hashes are retained in its `source.json` and in this investigation's validation evidence.

## Correctness and repeatability

`tools/gles-readback.cpp` checks 1,024 combinations against deterministic expected bytes, including untouched destination padding. It covers RGBA8, RGB565, RGBA4 and RGBA16F; odd small and multi-strip image sizes; mip levels and array layers; repeated content changes; full/subregion reads; row length, alignment and skips; reverse row order; and CPU/PBO destinations. Both original and patched ANGLE pass. The reusable host runner now includes this matrix alongside 216 topology cases and 32 independent plus 32 shared-context iterations, with Metal API Validation enabled and feature overrides cleared.

```sh
rtk proxy python3 tools/build-direct-gles-metal.py
rtk proxy python3 tools/check-direct-gles-metal.py \
  --artifact /absolute/path/printed/by/the/builder \
  --output evidence/metal-readback-host-new-run
```

Runtime launch and registration follow record 25. No additional feature override is required. Evidence is retained under `evidence/gles-tail-20261108/`.

## Controlled comparison

The final comparison uses the same GLES experiment VMM, Android disk/APKs, four vCPUs, manual 4 GiB RAM, MoltenVK for native Vulkan, and identical gfxstream library bytes. The control uses the original ANGLE libraries; the candidate differs by the bounded-strip readback patch. Each stack runs draw-call, fragment and texture workloads twice, reversing order on the second pass, with two seconds warmup and 30 seconds measured per run. Sampling, slow-call instrumentation, Metal validation and our builds are disabled during measurements. Percentiles are arithmetic means of run-level statistics, not pooled percentiles.

The complete twelve-run matrix passes correctness. The measured averages are:

| Workload | Control completions/s | Strips completions/s | Control p99 ms | Strips p99 ms |
| --- | ---: | ---: | ---: | ---: |
| Draw calls, 512² | 414.47 | 421.94 | 15.78 | 5.00 |
| Fragment, 2048² | 124.40 | 176.67 | 39.85 | 12.03 |
| Texture, 2048² | 180.99 | 114.66 | 13.95 | 43.74 |

The texture result is a measured regression in this matrix, not an improvement: candidate runs range from 53.12 to 176.20 completions/s and 19.50 to 67.97 ms p99. A separate candidate texture confirmation after the control and display tests reaches 180.98 completions/s with 16.05 ms p99 (`strips-texture-confirm/`), close to the control average. It remains supplemental and does not replace either original slow run. Both stacks have large isolated maxima (up to 270.80 ms for the control and 184.78 ms for the candidate across these workloads). Drawing and fragment tails improve in this run, but the result does not establish a universal throughput gain. `comparison.json` and both full pressure directories preserve every run.

This is a shared development host, not an isolated performance machine. Other GPU-capable applications and an ML job were active in the post-candidate process snapshot. That snapshot establishes concurrent load, not causation for a particular sample. The complete results retain the large second texture-run variation; no failed or slow samples are discarded.

These tests still measure offscreen single-frame completion with `glFinish`, not game FPS, native window latency or energy. Android composition and VMM display work can delay this timed loop even though the loop itself does not present or read pixels back; that interaction is the defect under investigation.

## Sustained display comparison

The offscreen matrix does not guarantee continuous composition, so a supplementary run targets the patched path directly: the same GLES 3 scene at 720×1080, 16 objects, 600 measured frames, `--finish-each-frame`, with host capture enabled. The original and patched ANGLE use the same VMM, driver, APK and request; the patched run follows the original after a VM restart. No validation layer or tracing is enabled. Both pass GLES pixel checks. These are guest render-loop rates, not physical Mac display FPS.

| Metric | Original row readback | Bounded strips |
| --- | ---: | ---: |
| Guest render-loop rate | 31.28 frames/s | 59.98 frames/s |
| Draw + `glFinish` mean | 20.43 ms | 2.02 ms |
| Draw + `glFinish` p99 | 48.99 ms | 5.76 ms |
| Guest frame-interval p99 | 50.41 ms | 22.50 ms |
| Captured host frames | 663 | 675 |

This directly supports the display-readback improvement. It does not override the mixed offscreen results or establish representative game performance. Exact requests and all timing statistics are retained in `display-comparison.json` and the two source reports.

## Independent-window lifecycle

The mixed Vulkan/GLES check exposed another defect: `open_target` fabricated the same display overlay on every window opening. Android treated each new fabricated overlay as changed global assets and recreated existing activities, invalidating the running Vulkan surface. Waiting after APK installation did not resolve the failure; logcat identifies the repeated `cmd overlay fabricate` and changed `.frro` path as the trigger. The runtime now checks the effective mirror-content resource and configures it only when needed, with an exclusive store lock around first-time setup and a checked postcondition. It does not cache a stale process-local flag.

Benchmark/stress fixtures also reuse an installed APK only after its single private base APK matches the bundled SHA-256. Missing, split or changed installations take the existing verified installation path. The mixed runner installs both fixtures before launching them, rejects stale `EXITING` windows and checks that opening the second display leaves the overlay unchanged. Failed installation-only and delay-only attempts remain in the evidence directory; they are not accepted regressions. The accepted fix retains strict Vulkan surface and pixel checks. A separate first-setup test disables the disposable overlay, confirms the effective value is `true`, opens a new GLES window, then verifies the effective value becomes `false` and captures 153 valid frames. This exercises both configuration and reuse paths.

## Functional acceptance

- Metal API Validation: 1,024 readback combinations, 216 topology cases, 32 independent-context and 32 shared-context iterations pass.
- Direct Vulkan on MoltenVK: ten feature/pixel/swapchain checks, one explicit swapchain recreation, 161 host frames; zero validation errors and warnings.
- Mixed GLES/Vulkan: two successful independent-display runs after the lifecycle fix. The final runner captures 614 Vulkan and 332 GLES frames, with 8.24 seconds of overlapping invocations and unchanged display-overlay identity.
- Independent input: all twelve checks pass across two guest displays, including held/repeated keys, multitouch, controller axes, focus reset, watchdog and close cleanup. Both displays advance 24 frames in the two-second observation. Native hardware input is not verified.
- Rust: 108 workspace tests pass; 17 opt-in tests are skipped by the default run. The relevant ignored independent-input test is run separately and passes. Workspace Clippy with `-D warnings` passes. Existing dependency future-compatibility notices remain.
- A second source-builder invocation verifies the pinned patch set and produces the same three library hashes.

## Test environment maintenance

The disposable VM reached Android's low-storage installation threshold: `/data` had approximately 483 MiB free, including 6.6 GiB of remount scratch and approximately 412 MiB of shell bugreports. The active remount scratch and app data were preserved. Nineteen older diagnostic ZIPs (225,164,551 bytes) were copied into `archived-guest-bugreports/`, verified against guest SHA-256 values, and only then removed from the disposable guest. Free space rose to approximately 698 MiB before the final matrices. The archive manifest records every original path, byte count and hash. User runtime storage was untouched.

## Local delivery and restoration

The independent development bundle is `dist/GlidePod-Readback-20261108.app`. Packaging verifies its ad-hoc signature; both bundled executable UUIDs and fixture APK hashes match the tested build. `tools/package-dev.py --output` now permits an isolated `.app` beneath this project's `dist` directory, rejecting external output before a build. This avoids overwriting the existing development app used by running background processes. No global installation or native UI acceptance is claimed. The direct ANGLE libraries remain separately registered experiment artifacts; opening the app does not silently enable them.

The disposable VM is stopped. Its original engine registration is restored exactly, its renderer preference is Software, MoltenVK remains selected, and both MoltenVK/KosmicKrisp registrations remain. The three saved configuration/registration files in the user's separate runtime are byte-identical to their initial hashes. The broad pre-existing worktree changes are preserved; no commit, push or publication was requested or performed. `validation.json`, `restored-runtime.json` and `package-verification.json` record the checks.

## Remaining boundaries

The direct backend remains an explicit ES 3.0 source experiment. Complete CTS, representative games, ES 3.1, external-buffer lifetimes and end-to-end native presentation require their separate qualification. Bounded CPU readback remains a copy-based display path and is not zero-copy interop.

Source reference: [pinned ANGLE readback implementation](https://chromium.googlesource.com/angle/angle/+/4d61277d5827d69d01934ef9930e935a34d07a19/src/libANGLE/renderer/metal/FrameBufferMtl.mm).

The subsequent [90/120 Hz follow-up](27-high-refresh-displays.md) removes display timing defaults and timer-based host capture limits; it retains this record's results as historical evidence.
