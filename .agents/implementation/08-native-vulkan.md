# Direct Android Vulkan validation

Date: 2026-10-23. This record extends [GLES 3 on Metal](07-gles3-metal.md). GLES through ANGLE and an application calling Vulkan directly are separate validation gates. The new fixture calls Android `libvulkan.so` from original NDK C++ code; it makes no EGL, GLES or ANGLE calls. Follow-up [swapchain tracing](09-vulkan-frame-pacing.md) corrects the interpretation of the recreation spike: producer acquisition waited while composed frames continued arriving.

## Implementation and use

`android/vulkan` contains an arm64 APK with a Java `SurfaceView`, a native worker, original GLSL/SPIR-V shaders, a private results provider requiring Android's `DUMP` permission, and cancellation when the surface closes. Fences/acquisition have five-second timeouts; the host terminates the workload after a 180-second deadline. A driver call that blocks internally is ultimately contained by the host stopping the app. The worker acquires an `ANativeWindow`, creates a Vulkan Android surface and presents to a swapchain on a GlidePod-managed non-default display. It uses the existing per-app display/frame transport and runtime epoch checks.

The Graphics page has a separate **Check Vulkan** action. CLI requests use the shared benchmark transport and report envelope with an explicit API field:

```sh
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-gpu-test --json benchmark --api vulkan
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-gpu-test --json benchmark --api vulkan --validation --samples 60
```

The ordinary check disables validation layers. `--validation` explicitly enables the packaged Khronos layer and synchronization validation; a missing layer is an error, never a silent fallback. The fixture requests up to Vulkan 1.3, checks the physical device capabilities, and enables only the optional features it exercises. Devices without timeline semaphores, Synchronization2 or dynamic rendering report those capabilities as unavailable and omit their optional checks. Advertised extensions, compression features and format flags are recorded separately from tested behavior.

Results retain the VM epoch, actual renderer, API request, independent display ID, raw guest samples, host copied-frame costs, and optional device timestamps. Failed guest diagnostics are saved as `developer/benchmark/latest-guest.json` before the CLI returns an error. Successful reports and captures remain `latest.json` and `latest.ppm`. The UI clears an old report when starting a new check. A hidden `--negative-control` option deliberately corrupts one returned texture pixel to test assertion/error propagation; it is never enabled by normal UI actions.

## Targeted checks

The evaluated Metal guest completes ten check groups:

1. Instance/device creation and a queue supporting graphics, compute and Android presentation.
2. Staging upload, storage-buffer compute, a compute-to-compute barrier between two dispatches, transfer and exact readback of 256 integers.
3. Synchronization2 transfer-write/read barriers and exact fill/copy readback.
4. Timeline semaphore submission, bounded wait and counter readback.
5. Image upload, descriptor binding and sampling: four expected RGBA quadrants, with two units of channel tolerance.
6. Depth comparison: a near red triangle must occlude two later, farther triangles at four pixel locations.
7. Indexed vertex-buffer geometry, push constants, depth-tested toruses and nonempty offscreen readback.
8. Vulkan 1.3 dynamic rendering with the same four-quadrant pixel assertions.
9. Android swapchain acquire, graphics submission, binary semaphore handoff and FIFO presentation. Host capture separately checks that colored scene pixels reach the independent display transport.
10. Explicit same-extent swapchain recreation during the workload, followed by continued presentation.

Graphics submissions use a completion fence and one submission in flight. Present-wait semaphores belong to swapchain images, rather than reusable frame slots. Superseded swapchain resources remain alive until workload teardown. The fixture handles out-of-date/suboptimal returns, but the deliberate recreation test keeps the extent unchanged; it does **not** validate live Mac-window resizing or every surface-loss/device-loss path.

## Reproducible inputs

```sh
rtk proxy python3 tools/prepare-android-tools.py
rtk proxy python3 tools/build-android.py
rtk proxy cargo build --workspace --locked
```

The existing toolchain preparation now fetches NDK r30 (30.0.16248370) and Khronos Android validation binaries 1.4.363.0. It verifies the official NDK archive SHA-1 and validation archive SHA-256. Native compilation targets Android API 29, arm64 and 16 KiB ELF alignment; shaders target Vulkan 1.1 SPIR-V and are embedded into the library. The build checks the validation library's SHA-256 before packaging it. APK assets retain the LLVM/Android runtime and Khronos license notices. Compiler tools are local build inputs and are not redistributed.

The registered Metal renderer is the same pinned gfxstream/MoltenVK 1.4.2 candidate and dynamic-topology patch evaluated in record 07. This Vulkan work does not replace the renderer or change user profile preferences. Software remains the automatic fallback. No external engine, guest image or renderer is added to the application bundle.

## Measurement protocol

```sh
rtk proxy python3 tools/evaluate-3d.py --data-dir .cache/runtime-gpu-test --api vulkan --repeat 2 --samples 180 --sustained-samples 3600 --output evidence/g6/metal
```

The evaluator requires a running disposable guest and a new output directory under `evidence`. It records renderer artifact hashes, host/guest identity and the exact APK hash. Five short cases cover 1/16/64 objects at 720×1080, 16 objects at 1080×1920, and 64 objects without Rust frame consumption, each repeated twice. An additional 3,600-frame heavy case exercises approximately one minute of rendering near 60 FPS. Each invocation repeats the correctness checks before two seconds of warmup. Validation layers are disabled for measurements.

`draw_ms` is CPU command recording/submission, not physical GPU duration. Frame intervals are between successful image acquisitions and include FIFO backpressure and fence completion; they are not display presentation intervals. Vulkan query timestamps are recorded separately, but their translation through gfxstream/MoltenVK has not been calibrated against Metal hardware instruments. They must not be presented as independently verified physical GPU times. In the preserved G6 evidence, explicit swapchain recreation cost is reported separately and the interval across the recreation call is omitted. Subsequent producer delays remain in the recorded interval series and tail statistics. The G7 implementation includes the lifecycle interval as well.

VMM CPU includes Android CPU work, renderer and initialization/checks; 100% means one logical CPU. Host capture measures file-backed frame reading and RGBA/BGRA conversion. Disabling that consumer does not disable upstream gfxstream readback. GPUI texture upload, WindowServer and physical display latency are outside this measurement. This indexed Vulkan scene also differs from the GLES fixture in submission, texture and geometry layout, so the records do not establish a Vulkan-versus-GLES speed ratio.

## Evidence and remaining gates

The final Metal validation run (`evidence/g6/metal-validation.json`, local output) uses core and synchronization validation and reports zero errors and zero warnings, including teardown. The negative control (`evidence/g6/negative-control.json`, local output) proves that a wrong pixel returns failure with empty stdout and retained diagnostic detail. The guest capture (`evidence/g6/native-vulkan-first.png`, local output) shows actual Vulkan toruses through the independent display transport; it is not a native macOS screenshot.

The Software validation run (`evidence/g6/software-validation.json`, local output) also passes all ten groups with zero errors and zero warnings. Metal reports Vulkan 1.4.357 and Software reports Vulkan 1.3.0; both checks explicitly request Vulkan 1.3.0. Metal advertises geometry shaders as unavailable. ASTC/ETC2/BC compression bits are inventoried, but compressed texture decoding has not been tested.

The Metal matrix (`evidence/g6/metal/report.json`, local output) and Software matrix (`evidence/g6/software/report.json`, local output) each contain eleven runs. All 22 invocations pass the ten check groups, covering 10,800 measured frames (`evidence/g6/matrix-verification.json`, local output). Both guests use four vCPUs and 4 GiB RAM on Apple M5 Max/macOS 27.0.1. Matrices run sequentially, without builds, other guest suites or the other VM running concurrently. The same APK is used in both matrices and in the packaged app.

| Workload | Renderer | Mean FPS | Frame p95 (ms) | VMM CPU (%) |
| --- | --- | --- | --- | --- |
| 16 objects, 720×1080 | Metal | 59.78–59.85 | 21.43–22.27 | 43.8–48.4 |
| 16 objects, 720×1080 | Software | 59.74–59.80 | 18.40–18.47 | 83.1–84.4 |
| 64 objects, 720×1080 | Metal | 59.85 | 21.72–21.94 | 41.4–47.5 |
| 64 objects, 720×1080 | Software | 59.86–59.89 | 17.82–17.98 | 109.9–110.5 |
| 16 objects, 1080×1920 | Metal | 58.82–59.71 | 21.42–21.90 | 44.5–46.6 |
| 16 objects, 1080×1920 | Software | 59.79–59.83 | 17.89–18.44 | 85.5–89.4 |
| 64 objects, 3,600 frames | Metal | 59.99 | 21.71 | 51.6 |
| 64 objects, 3,600 frames | Software | 59.94 | 17.99 | 193.0 |

The short rows span two runs; each long row is one approximately 60-second run. CPU includes initialization/warmup and the entire VMM invocation, so short and long CPU averages are not interchangeable. These results establish lower VMM CPU for Metal in this fixture, not a power ratio or a higher maximum FPS. Both modes are refresh-limited. Metal has worse frame p95 here.

A repeatable producer interval spike appears around the fourth measured frame after swapchain recreation in both modes. Metal's long run has an 81.86 ms interval at sample 1,803; Software has a 47.46 ms interval there, plus two later outliers (35.46 and 74.37 ms). The corresponding explicit recreation calls take only 5.01 and 2.10 ms. A second Metal 1080p run also has three additional slow intervals after recreation. The raw samples retain every outlier. The original interpretation as a presentation interruption was too strong: [G7 traces](09-vulkan-frame-pacing.md) locate the recreated-swapchain spike in Android BufferQueue acquisition and demonstrate continuous SurfaceFlinger latching and host content arrival. The other G6 outliers were not retrospectively traced; they remain unclassified. These producer statistics establish neither Metal-specific display stutter nor universally smooth presentation.

Simultaneous Vulkan and GLES 3 (`evidence/g6/concurrent-displays/report.json`, local output) pass on the packaged Metal runtime with distinct display IDs, 583/282 captured frames and overlapping invocations. This is transport/display isolation evidence; it is not a visual proof of two macOS windows. APK installs are deliberately serialized before overlapping workloads, respecting the existing installer lock. The initial harness attempted simultaneous installation and was correctly rejected by that lock; the corrected reusable script waits for the first package to launch.

```sh
rtk proxy python3 tools/check-vulkan-displays.py --data-dir .cache/runtime-gpu-test --output evidence/new-vulkan-display-run --cli dist/GlidePod.app/Contents/MacOS/glidepodctl
```

Representative commercial games, CTS/conformance, compressed-texture correctness, advanced extension coverage, long thermal/power testing and native window/device acceptance remain separate gates. Reported API version and advertised feature bits do not certify those capabilities. The desktop-access limitation from earlier records still prevents a claim of native button or physical display validation.

## Delivery

The packaged 1080×720 landscape check (`evidence/g6/packaged-landscape-validation.json`, local output) passes all ten groups with core/synchronization validation and zero errors/warnings. It verifies a second display orientation at creation time, not live window resizing. The packaged CLI also runs the simultaneous Vulkan/GLES display check successfully.

Bundle validation (`evidence/g6/bundle-validation.json`, local output) records 46 passing workspace tests, formatting, strict Clippy, Android Java/C++/shader compilation, Python syntax checks and deep/strict ad-hoc signature verification. Four older real-device integration tests remain ignored in the ordinary unit run; this record does not claim they were rerun. The new direct Vulkan suites, concurrent GLES 3 regression and packaged landscape check are separate real-guest evidence.

The exact measured Vulkan APK is included in `dist/GlidePod.app`; build provenance (`evidence/g6/native-build.json`, local output) records source and library hashes. The app was relaunched from that bundle and its new live process path was verified. Desktop inspection returned `cgWindowNotFound`, so the native **Check Vulkan** button has not been clicked through automation. All evaluation VMs are stopped, and cleanup (`evidence/g6/cleanup.json`, local output) confirms that the user's configuration is unchanged. No release was published.

## Primary references

- [Android NDK downloads and official archive pattern](https://developer.android.com/ndk/downloads).
- [Khronos validation layer release 1.4.363.0](https://github.com/KhronosGroup/Vulkan-ValidationLayers/releases/tag/vulkan-sdk-1.4.363.0).
- [Khronos swapchain semaphore reuse](https://docs.vulkan.org/guide/latest/swapchain_semaphore_reuse.html) and [swapchain recreation](https://docs.vulkan.org/samples/latest/samples/api/swapchain_recreation/README.html).
