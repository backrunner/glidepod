# GLES 3 on the experimental Metal renderer

Follow-up: [direct Android Vulkan validation](08-native-vulkan.md) now covers a separate native Vulkan workload. This record retains the earlier GLES measurements.

Date: 2026-10-23. This record extends [3D, storage and notifications](06-3d-storage-and-notifications.md). The scope is the registered Developer runtime on Apple M5 Max/macOS 27.0.1. It is not a general game-compatibility or Khronos conformance claim.

## Implementation

The original MoltenVK 1.4.0 path exposed GLES 2 only. ANGLE requires last-vertex provoking behavior for GLES 3 flat interpolation. MoltenVK 1.4.1 introduced a real implementation behind `MVK_USE_METAL_PRIVATE_API`; the evaluated candidate now uses source-built **1.4.2**, commit `db66022459ffb663aa2b50f6b018bc2e124f5edf`, with that option enabled. The guest ANGLE and gfxstream transport remain unchanged. No guest layer fabricates an extension, no GLES version is forced and no shader interpolation requirement is removed.

The project explicitly targets distribution outside the Mac App Store. This optional driver uses private Metal selectors and therefore requires renewed OS/device compatibility checks. It is a measured experimental path, not a promise that private interfaces remain stable. The existing Software fallback and original renderer artifacts remain available.

## A failure discovered during validation

Creating an ES 3 context succeeded on the unpatched 1.4.1 and 1.4.2 drivers, which reported OpenGL ES 3.1 on Apple M5 Max. A targeted fan test nevertheless rendered only the first of three triangles. The same test passed on Software. The failure is preserved in the original result (`evidence/g5/v142-correctness-first.json`, local output) and pixel capture (`evidence/g5/flat-triangle-fan-failure.png`, local output).

MoltenVK's fan emulation selected its path using the topology from pipeline creation. ANGLE can switch topology dynamically. Both lists and fans map to Metal's triangle primitive, so keeping only the Metal type loses the information needed to expand a fan. The local patch preserves Vulkan topology in static/dynamic state and selects the effective topology in all four direct/indexed/indirect draw paths. It does not change advertised capabilities.

The reviewable [patch](../../engine/patches/moltenvk-dynamic-triangle-fan.patch) and [explanation](../../engine/patches/README.md) live outside ignored caches. This patch has not been submitted upstream. New GlidePod code and patch contributions use the project's Apache-2.0 license; upstream files retain their original notices.

## Correctness and user-facing checks

The original benchmark APK now has 13 targeted checks on the observed ES 3.1 context:

- Flat interpolation from the last vertex; reversed index ordering; triangle strips.
- Direct, indexed, array-indirect and indexed-indirect triangle fans, with different expected colors in each fan segment.
- Indexed primitive restart and topology transitions.
- Integer instance attributes, vertex array objects and a uniform buffer.
- Multiple render targets with independent pixel readback.
- Array texture layer sampling, four-sample resolve and transform-feedback buffer readback.

The two indirect checks require ES 3.1 and run when that context version is present. Each pixel assertion uses known colors with a two-unit channel tolerance. Shader compilation/linking, GL errors and mapped-buffer contents are checked. Failed pixel checks retain their RGBA image in the JSON. ES 3 context creation and rendering correctness are separate results.

**Graphics → Check GLES 3** runs the scene with an explicitly requested ES 3 context and GLSL ES 3 shaders. The ordinary 3D check retains its ES 2 request for comparison and also probes ES 3 correctness. The CLI supports `benchmark --gles 3`; an unavailable configuration reports a bounded error, and failed correctness leaves the diagnostic report on disk and returns failure. The UI distinguishes an available context from passed checks.

## Reproduction

```sh
rtk proxy python3 tools/build-metal-renderer.py
rtk proxy target/debug/glidepodctl register-gpu /absolute/path/printed/by/the/build
rtk proxy target/debug/glidepodctl configure --renderer metal
```

Registration and renderer changes require Android to be stopped. The build script verifies the [lock file](../../engine/metal-renderer-lock.json), source commit, exact local patch, dependency revisions, downloaded CPM digest and previously evaluated gfxstream/loader input hashes. It produces an immutable directory, keeps original/dependency license texts, and records provenance. A cache hit rechecks artifact hashes. It neither replaces a live renderer nor automatically modifies user preferences. The exact base binary hashes currently tie this experiment to the recorded local inputs; a different base build needs a reviewed lock update.

After starting a disposable registered Developer guest:

```sh
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-gpu-test --json benchmark --gles 3 --objects 64 --samples 180
rtk proxy python3 tools/evaluate-3d.py --data-dir .cache/runtime-gpu-test --gles 3 --repeat 2 --output evidence/g5/metal
```

Evaluation output must be new and under `evidence`; previous measurements cannot be overwritten. Reports include the registered renderer hashes and requested GLES version. The isolated suite measures guest draw/frame timing and copied-frame consumption. GPU-only timers, GPUI presentation, sustained thermal performance, Vulkan/GLES conformance and representative game qualification remain separate gates.

## Evidence and remaining limits

The patched bring-up passed all 13 checks (`evidence/g5/patched-indirect-correctness.json`, local output). The final Metal matrix (`evidence/g5/metal/report.json`, local output) runs an ES 3 context and GLSL ES 3 shaders, with all 13 checks repeated in every case. All four real-guest regression suites (`evidence/g5/regression/report.json`, local output) also passed on the final renderer: app lifecycle, independent input, host focus leases, and storage/permissions/notification management. The old-renderer negative control (`evidence/g5/unsupported-gles3.json`, local output) rejected an ES 3 request in 1.86 seconds with an empty stdout and structured error. Native notification banners/clicks and physical input devices remain subject to the desktop-access limitation recorded in the previous implementation notes.

### Final performance comparison

Metal and Software (`evidence/g5/software/report.json`, local output) ran sequentially on the same M5 Max, each with four vCPUs, 4 GiB guest RAM, two seconds of warmup and 180 measured frames per case. Each renderer completed 12 runs. Every run also passed the 13 correctness checks. No builds or other guest tests ran concurrently with the matrices. The measured and packaged benchmark APKs are byte-identical.

| Workload | Renderer | Mean FPS | Frame p95 (ms) | VMM CPU (%) |
| --- | --- | --- | --- | --- |
| 16 objects, 720×1080 | Metal | 60.01–60.03 | 21.55–22.21 | 46–48 |
| 16 objects, 720×1080 | Software | 59.96–60.01 | 20.33–20.44 | 126–173 |
| 64 objects, 720×1080 | Metal | 59.95–59.97 | 20.87–21.31 | 47–50 |
| 64 objects, 720×1080 | Software | 60.00–60.01 | 19.68–20.54 | 192–193 |
| 16 objects, 1080×1920 | Metal | 59.99–60.01 | 21.35–22.11 | 49–50 |
| 16 objects, 1080×1920 | Software | 60.00–60.02 | 20.13–20.54 | 141–142 |

Ranges span the two repetitions. CPU covers the entire VMM during each invocation, including correctness checks/startup/warmup, and 100% is one logical core. It is not GPU utilization. The wide Software CPU range in the medium case reinforces the limits of two short runs; do not interpret it as a stable power ratio. This matrix's ES 3 shaders and pre-run checks differ from record 06, so CPU figures are not a controlled before/after comparison with that record.

All 24 runs have zero measured intervals above 33.33 ms. Both modes remain near the refresh ceiling; no maximum-throughput or Metal FPS advantage is established. Metal uses less VMM CPU in these runs, while its frame p95 remains worse. Hardware GPU timing, native presentation latency and sustained/game workloads are still open. The heavy scene capture (`evidence/g5/metal/heavy-720-1.png`, local output) is a real guest frame, not a native-window screenshot.

### Delivery

Formatting, strict Clippy, 46 workspace tests, CLI contracts, Android compilation, packaging and deep/strict ad-hoc signature checks passed. The source-built renderer and its cache reuse were both verified; exact inputs, flags and notices are in build provenance (`evidence/g5/build/provenance.json`, local output). The patched external renderer is now registered in the app's default store. Existing profile/resource/audio/notification preferences are unchanged; selecting Developer and Metal uses it on the next VM start. Automatic continues to use Software.

The updated app was relaunched from `dist/GlidePod.app`; executable hashes and the live process path are recorded in bundle validation (`evidence/g5/bundle-validation.json`, local output). The final desktop inspection again returned `cgWindowNotFound`, so the new native button and notification interaction remain unverified. All disposable evaluation VMs are stopped; see cleanup (`evidence/g5/cleanup.json`, local output). The external VMM/renderer are not bundled into the app or published as a release.

## Primary references

- [MoltenVK 1.4.1 changes](https://github.com/KhronosGroup/MoltenVK/blob/v1.4.1/Docs/Whats_New.md) and [private Metal option](https://github.com/KhronosGroup/MoltenVK/blob/v1.4.2/README.md#accessing-metal-private-api-calls).
- [Pinned MoltenVK draw implementation](https://github.com/KhronosGroup/MoltenVK/blob/db66022459ffb663aa2b50f6b018bc2e124f5edf/MoltenVK/MoltenVK/Commands/MVKCmdDraw.mm).
- [Khronos GLES specifications](https://registry.khronos.org/OpenGL/index_es.php). Targeted fixture passes are not conformance certification.
