# Upstream audit and bounded frame presentation

Date: 2026-11-05. Scope: current upstream metadata, compatible Rust dependency updates, host frame processing and a follow-up plan for the external runtime. External runtime pins are unchanged.

## Dependency decisions

| Component | Existing baseline | Checked upstream | Decision |
| --- | --- | --- | --- |
| GPUI | 0.2.2 | 0.2.2 stable on crates.io | Retain. |
| gpui-component | 0.5.1 | 0.9.0 | Defer a coordinated UI migration. The published 0.9.0 manifest depends on `gpui-pre =0.3.11`, `gpui-base` and kit assets; it is not interchangeable with this application's `gpui 0.2.2` types. Even 0.6.6 uses `gpui-pre`. |
| sha2 | Direct dependency 0.10.9 | 0.11.1 | Update direct consumers. Adapt digest hexadecimal formatting with the existing graph's `hex` crate. The old SHA version remains transitively required. |
| base64 | Direct dependency 0.22.1 | 0.23.1 | Update; preserve the existing alphabet, padding and GeneralPurpose engine behavior. The new release's optional SIMD engines are not automatically used by those call sites. |
| zip | 2.4.2 | 9.3.0 | Update; handle fallible entry-name decoding. Enable only `deflate-flate2-zlib-rs`, removing the unused Zopfli compression dependency. Keep APK size, central-directory, path, ABI and manifest checks. |
| Other Rust packages | Cargo.lock | Compatible stable resolution | Refresh the lock, including cc 1.11.0, async-executor 1.14.1, async-lock 3.5.0 and smallvec 1.16.4. Regenerate dependency notices. |
| MoltenVK | Patched 1.4.2 (`db66022`) | 1.4.2 remains latest stable | Retain the reviewed dynamic triangle-fan patch. The 1.4.0 in the original Avo evaluation lock is historical reference metadata, not the active renderer pin. |
| Vulkan-Loader | v1.4.365 | v1.4.367 (`aa6dee8385afe4b2cc0152d086af2184743e16fd`) | Candidate for a source-built renderer batch; do not alter hashes of existing binaries. |
| gfxstream | `ec3143b4ac345af198b3f5690bfc79c728128704` | `94240f814c0097a63027ae680ea4bec7b0cb25f7`, 212 commits ahead | Candidate; requires matching Avo patches, guest protocol and GLES/Vulkan validation. |
| Avo | `99153258763a9ccdf5d4c6de36bd631e4dd0dded` | `c8f741d03965e51b9e4a33e0f62db64738c21b25`, 278 commits ahead | Candidate; frame ABI and resource ownership changed. No automatic replacement of the registered engine. |
| Cuttlefish guest | Build 16644040, ARM64 phone userdebug | Latest successful submitted build on `aosp-android-latest-release` is still 16644040 | Retain. The query covers this branch/target, not every Android build. |

Sources: [crates.io package API](https://crates.io/api/v1/crates/gpui-component), [0.9.0 dependencies](https://crates.io/api/v1/crates/gpui-component/0.9.0/dependencies), [gpui-kit release](https://github.com/longbridge/gpui-kit/releases/tag/v0.9.0), [MoltenVK release](https://github.com/KhronosGroup/MoltenVK/releases/tag/v1.4.2), [Loader tags](https://github.com/KhronosGroup/Vulkan-Loader/tags), [Avo comparison](https://github.com/nizwar/avo/compare/99153258763a9ccdf5d4c6de36bd631e4dd0dded...c8f741d03965e51b9e4a33e0f62db64738c21b25), [gfxstream comparison](https://github.com/google/gfxstream/compare/ec3143b4ac345af198b3f5690bfc79c728128704...94240f814c0097a63027ae680ea4bec7b0cb25f7). Cuttlefish was queried through the public v4 Android Build API with `branches`, `targets`, `buildAttemptStatus=COMPLETE`, `buildType=SUBMITTED`, `successful=true` and `pageSize=1`, as in the [official fetch client](https://android.googlesource.com/tools/fetch_artifact/+/refs/heads/androidx-appfunctions-release/fetch_artifact.go).

## Implemented host changes

Previously, a BGRA transport frame was converted to RGBA in the reader and then back to BGRA on the UI thread. The reader now accepts an explicit output pixel format. GPUI requests BGRA on its worker; screenshots and diagnostic callers still request RGBA. Source BGRA requires no swizzle for presentation, and source RGBA requires only one. Alpha is preserved, and layout, generation, slot and post-read header checks remain enforced.

The UI now consumes its single pending frame only during actual rendering. It replaces the current image and evicts the previous GPUI image at that point. The old timer-driven `retired` vector had no bound if view updates continued while rendering was suspended. The new worker waits on a condition variable while its one slot is occupied; consumption wakes it immediately, and a bounded timeout observes shutdown. Poll pacing accounts for read time instead of adding a full delay after processing.

For a frame payload of S bytes, application-owned steady frame storage is one current image plus one pending frame (2S), with temporary overlap during replacement. At 1080×1920, 2S is approximately 15.82 MiB. This is an ownership bound, **not** a total RSS measurement: GPUI scene references/atlas textures, renderer staging, guest RAM and macOS accounting are separate. The VMM still performs its existing readback and triple-buffer publication; consumer backpressure does not disable guest rendering or eliminate that upstream cost.

Benchmark reports identify the new path as `copied_bgra_normalized_during_read`. `host_read` includes any required normalization, while `host_bgra_conversion` is null because there is no separately timed second conversion. Historical reports retain their original meaning. Compare old read plus conversion cost with the new combined read, not the two read fields alone.

## Host microbenchmark

Apple M5 Max, 64 GiB RAM, macOS 27.2 (26B123), Rust 1.99.1. Both paths run in the same optimized development runtime against the same cached synthetic BGRA transport file. Each run alternates path order, warms up 30 iterations, then measures 300 frames per path. Three runs were executed without a concurrent build or guest. Table values are medians of the three p50 values.

| Frame size | Previous read plus two swaps | Direct BGRA read | Lower processing time |
| --- | ---: | ---: | ---: |
| 720×1080 | 0.953 ms | 0.104 ms | 89.1% |
| 1080×1920 | 2.604 ms | 0.432 ms | 83.4% |
| 2048×2048 | 5.448 ms | 1.058 ms | 80.6% |

These measure host allocation, cached file copying, header validation and pixel normalization. They exclude live session IPC, guest rendering, GPUI texture upload, WindowServer and physical display latency. They are not game FPS or total application speedup claims. Run them separately from other workloads:

```sh
cargo test -p glidepod-runtime --locked --lib display::tests::copied_frame_benchmark -- --ignored --nocapture
```

Raw API snapshots, source inspection and microbenchmark logs are under the ignored `evidence/upstream-2026-11-05/` directory. They are local evidence, not shipped runtime assets.

## Validation and remaining limits

- Formatting, workspace Clippy with all targets and `-D warnings`, and workspace tests pass: 50 tests passed, 5 explicitly ignored. One ignored test is the separately executed microbenchmark. Cargo still reports existing future-incompatibility notices for transitive `block 0.1.6` and `proc-macro-error2 2.0.1`; this run does not establish future Rust or minimum-Rust compatibility.
- Android helpers and the native development bundle were rebuilt, and the ad-hoc bundle signature verified. The packaged ARM64 Hypervisor execution and Metal compute/readback probes pass. APK preflight's SHA-256 matches an independent Python hash after the sha2 migration.
- In a new isolated store, the unchanged pinned VMM and renderer boot Cuttlefish 16644040 with four vCPUs and 4 GiB RAM. The opt-in real guest lifecycle and independent window/input suites both pass, including signed install/update, unsigned rejection, Unicode round-trip, input isolation and package-scoped task teardown. Their output was moved into this audit's evidence directory while preserving earlier runs.
- The GLES 3 fixture passes all 13 targeted checks, and direct Vulkan with validation enabled succeeds with zero reported validation errors. The new BGRA host consumer receives 274 and 285 frames respectively. This confirms the new transport consumer against real guest rendering; it is not a repeated game-performance comparison.
- The development UI starts with two Android fixture sessions. A separate sampling profile finds both frame workers waiting on the bounded pending-frame condition variable. Across 11 samples spanning 20 seconds in this stalled-consumer state, UI RSS stays at 122,976 KiB. The profile reports a 73.5 MiB physical footprint at its separate sample time. Neither value includes the VMM, proves a long-run plateau during visible rendering, nor measures a before/after RSS improvement.
- Native visual/minimize/restore validation remains incomplete: the computer-use provider returns `cgWindowNotFound` for the app path, bundle ID and display name. No screenshot or physical display result is claimed. The dependency/runtime migration gates above remain separate work; this change has not been committed, pushed or published.

## Next runtime work, in priority order

1. **Qualify Avo's readback and shared-memory changes as one candidate.** [Off-thread readback](https://github.com/nizwar/avo/commit/21bae7b7fdf7537eb191d09eb268d0292883e561) adds gfxstream symbols and imports aligned host memory, while [queue notification suppression](https://github.com/nizwar/avo/commit/288eb29592b953fd75404c0541ce7e651a2e2b81) reduces needless guest exits. [POSIX shared memory](https://github.com/nizwar/avo/commit/c94bd7cc1c11f6787b8fc8d8333bf2de02967ccf) avoids file-backed frame/audio writeback. The newer frame offset is 16 KiB and announcements use `shm:<name>`; the current reader deliberately accepts an owned file with a 4 KiB header offset. Add versioned negotiation, bounded layout validation, owner/epoch checks, read-only mappings and deterministic unlink/cleanup before switching. Hold mappings through in-flight reads and reject stale generations. Build source and patches in a separate immutable candidate; preserve the existing fallback. Re-run two-window input, resize/restart, GLES 3 fan checks, Vulkan correctness and repeated content-gap captures. Shared readback memory still is not direct IOSurface presentation.
2. **Move blocking disk operations off vCPUs.** Inspection of [Avo's current block device](https://github.com/nizwar/avo/blob/c8f741d03965e51b9e4a33e0f62db64738c21b25/engine/hvf/vmm/src/virtio/block.rs) still finds normal queue serving inside `notify` and synchronous `disk.flush()`. Its disk-full retry worker is not general asynchronous block I/O. Apply the existing [ordering, reset, failure and durability plan](10-async-block-io-plan.md); do not suppress fsync or acknowledge work early to improve FPS. Compare guest runnable delays and whole-run host content gaps under concurrent storage activity.
3. **Prototype native surface presentation after transport correctness.** Use a bounded IOSurface/CVPixelBuffer pool with explicit producer completion, consumer ownership and generation retirement. The installed GPUI has a macOS surface-painting API, but safely importing the external renderer's output still requires engineering and measurements. Retain the copied path for diagnostics and unsupported layouts. Measure copies, CPU time, upload time, physical footprint, GPU allocations and p95/p99 content intervals together.
4. **Keep resource budgets explicit.** Retain the current 4 vCPU/4 GiB default until workload measurements justify a change. Sweep 2/4/6 vCPUs and 2/4/6 GiB with the same workload, then measure one/two/four app windows, occlusion/minimization, restoration and teardown. Check host pressure and compression rather than treating guest configured RAM as resident usage. Native UI timers can be reduced further when hidden, while preserving input release and close handling. Acceptance requires bounded memory after warmup and recovery after pressure, not only one fast benchmark.

Keep the UI framework migration separate from the external VMM/renderer batch so regressions remain attributable. No new GPU extensions, game compatibility or graphics conformance are claimed by this work.
