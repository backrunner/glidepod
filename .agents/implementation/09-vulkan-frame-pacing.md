# Vulkan swapchain recreation and frame pacing

Date: 2026-10-23. This follows [direct Android Vulkan validation](08-native-vulkan.md) and corrects its original interpretation of an approximately 80 ms interval after swapchain recreation.

## Finding

The reproduced spike is **producer backpressure in Android BufferQueue acquisition**, while SurfaceFlinger continues latching frames and new scene content continues reaching the host scanout. Calling that spike an 80 ms display interruption was not supported by the earlier measurements.

The native fixture records intervals between successful `vkAcquireNextImageKHR` calls. After recreation, the four new images can initially be acquired and submitted quickly. Acquiring the fifth waits for an image to become available. During that wait, previously queued frames continue through the consumer. The interval series therefore contains a short burst followed by a long wait even when output content continues at the normal cadence.

In the final Metal trace, the large acquire takes 77.64 ms. Its nested Android `dequeueBuffer` accounts for 77.62 ms, while five SurfaceFlinger latch events occur during that acquire. The explicit recreation call takes 2.51 ms. The host sees every numbered scene frame around recreation, with no repeated-content gap approaching 80 ms. No renderer sleep, present-mode change, or workaround to suppress the outlier was introduced.

## Evidence

All traced runs use a disposable Developer guest with four vCPUs and 4 GiB RAM, the pinned Cuttlefish build 16644040, and the same external renderer candidate as G6. Metal and Software runs are sequential. The workload is 16 objects at 720×1080, two seconds of warmup and 180 measured producer intervals. Validation layers are disabled during these captures.

| Final fixture run | Largest measured producer interval | Selected acquire wait | Largest host content interval near recreation | Whole measured run: largest observed host interval | Largest SurfaceFlinger latch interval nearby |
| --- | ---: | ---: | ---: | ---: | ---: |
| Metal recreation (`evidence/g7/metal-final/analysis.json`, local output) | 78.33 ms | 77.64 ms | 18.70 ms | 40.35 ms | 19.49 ms |
| Software recreation (`evidence/g7/software-final/analysis.json`, local output) | 66.19 ms | 61.29 ms | 21.46 ms | 21.61 ms | 21.26 ms |
| Software control, no recreation (`evidence/g7/software-control/analysis.json`, local output) | 22.49 ms | 19.14 ms | 21.45 ms | 21.53 ms | 20.04 ms |

The no-recreation control uses the middle of the measured run as its comparison window. The selected acquire is the largest retained stage sample, which need not correspond to the largest full-series producer interval. The Metal recreation run has two observed host intervals above 33.33 ms elsewhere, with polling delays retained in its report. The packaged Metal control (`evidence/g7/packaged-metal-control/analysis.json`, local output) independently covers the same no-recreation path from the delivered application bundle, and captures a **different, real host-delivery gap** outside that middle window, described below. Recreation-window maxima must not be interpreted as whole-run smoothness.

Two earlier instrumented Metal repetitions also reproduce the same queue behavior: run 1 (`evidence/g7/metal-trace-1/analysis.json`, local output) has a 76.24 ms producer interval with at most 18.48 ms between host content observations nearby; run 2 (`evidence/g7/metal-trace-2/analysis.json`, local output) has 74.33 ms and 20.91 ms respectively. Both have five latch events during the long acquire. Those two runs still used the old metric that omitted the interval directly spanning recreation. The final fixture includes that interval; preserved earlier evidence is not rewritten.

Each capture directory retains:

- The raw guest Perfetto trace, exact trace configuration, benchmark report and log.
- Host scanout observations with monotonic timestamps, scanout identity, sequence number and decoded scene-frame number.
- Every host polling timestamp, so reader scheduling delays are inspectable.
- Extracted acquire/dequeue, swapchain lifecycle, SurfaceFlinger latch and clock-offset CSVs.
- An analysis report with trace SHA-256, continuity checks and parser diagnostics.

The trace contains Android WSI `disconnect` and `connect` calls during recreation, followed by buffer allocation and the initial acquisition burst. This is consistent with [AOSP's Vulkan swapchain implementation](https://android.googlesource.com/platform/frameworks/native/+/refs/heads/main/vulkan/libvulkan/swapchain.cpp), which reconnects an already used native window and delegates acquisition to `dequeueBuffer`. The inspected main-branch source is explanatory reference material, not a proven source match for guest build 16644040. Its locally retrieved SHA-256 is `98f4c3b36a90114f9a555c7fae2814be131ee96358881b320aeb70e73e6786dc`; the runtime attribution comes from the captured call nesting.

## Measurement boundaries

The host observer reads the existing copied, triple-buffered frame transport. In diagnostic mode only, the Vulkan fixture adds an 8×8 pixel marker encoding the scene frame number. A changed transport sequence alone could be another copy of the same image; the marker identifies changed application content. The observer checks the header before and after reading and rejects a slot/sequence mismatch. It polls at a requested 2 ms cadence. Actual maximum polling gaps in the final recreation comparison windows are 3.20 ms on Metal and 3.31 ms on Software. These are sampled arrival times, not exact writer publication timestamps.

The comparison window covers frames four before through eight after the first recreated frame. Both final recreation runs contain every frame in order throughout that window. SurfaceFlinger latch events independently show the consumer advancing while acquisition is blocked. Guest `CLOCK_MONOTONIC` values are aligned to Perfetto's trace clock using clock snapshots. Host and guest absolute timestamps are not assumed to share an epoch; the content frame IDs establish the host-side ordering.

The guest Perfetto service reports v54.0; analysis uses the official v58.2 trace processor (`add693d8b338ba9599dbcbc3e300b1ab8c000897`). The optional FrameTimeline source produces parser errors in these captures. Its jank classifications and presentation metrics are **not used**. The conclusion uses raw ATrace/ftrace call nesting and latch events plus the independently sampled content markers. The analyzer retains parser, discarded-chunk and overwrite diagnostics. Later extended kernel captures have additional limitations described below; their absence of an event is not treated as proof that the event did not occur.

This establishes continuity through guest composition and host copied-frame delivery for the reproduced recreation event. It does not measure GPUI texture upload, WindowServer presentation, input-to-photon latency, or the physical Mac display. The additional non-recreation outliers in the old one-minute G6 runs remain unclassified because they were not traced. Commercial-game smoothness, resize/surface-loss stress and long thermal testing remain separate acceptance work.

## Separate steady-state gap and storage investigation

The packaged Metal control (`evidence/g7/packaged-metal-control/outlier.json`, local output), without deliberate recreation or a host sampling profiler, has a 46.61 ms producer interval and a 48.31 ms host content-arrival interval. Host polling continues through that gap with a maximum 3.13 ms polling interval. Transport sequence 175 is skipped by the observer between 174 and 176, so frame 168 was not observed; this is not proof that the renderer never produced that frame. Closely spaced publications after the gap can explain the missed observation.

Unlike the recreated-swapchain burst, this event includes delayed composition. SurfaceFlinger scheduling (`evidence/g7/packaged-metal-control/surfaceflinger-wakeup.csv`, local output) shows RenderEngine waking the main compositor, followed by 30.12 ms in runnable state before it executes. CPU scheduling (`evidence/g7/packaged-metal-control/outlier-cpu-scheduling.csv`, local output) places `kworker/0:3H` on virtual CPU 0 for 30.30 ms across that interval, while other virtual CPUs continue executing. A 31.19 ms `composeSurfaces` wall-clock slice and a roughly 30 ms completion-fence wait overlap this period. Neither number can be called physical GPU rendering time.

A subsequent 20-second host sample (`evidence/g7/vmm-host-sample.txt`, local output) observes all four VMM vCPU threads inside `Block::notify → CompositeDisk::flush → File synchronization → fcntl`. The inspected pinned VMM source processes block requests synchronously in `notify`; `T_FLUSH` synchronizes writable partitions before returning to guest execution. This confirms a host I/O path that can block guest CPU scheduling. It is a strong candidate for these separate scheduling hitches, but the aggregate host sample is not a timestamp-aligned proof of the exact earlier 48.31 ms event. VMM provenance (`evidence/g7/vmm-provenance.json`, local output) records the executable and relevant source hashes.

The host-profiler capture increases overhead and is **not** a comparable performance run. Two early extended kernel traces also omit kernel symbol resolution, producing interning diagnostics; the first also overwrites part of its 32 MiB ring buffer. They are retained as diagnostic attempts, not clean kernel-attribution evidence. The capture tool now enables `symbolize_ksyms`, as required by [Perfetto's kernel symbolization guidance](https://perfetto.dev/docs/learning-more/symbolization), and uses a larger trace buffer. A subsequent symbolized capture identifies `blk_mq_requeue_work` on `kworker/0:3H`, including a 47.87 ms work item during setup. Some service chunks are still reported discarded, so the analyzer conservatively marks trace integrity as unqualified. Positive retained call records can be inspected; missing events and absolute event counts are not acceptance evidence.

In that 600-interval symbolized run without host sampling (`evidence/g7/metal-symbolized-control/analysis.json`, local output), the independently observed measured host-content intervals peak at 24.51 ms with none above 33.33 ms. This demonstrates that the earlier gap is intermittent, not that storage-induced scheduling stalls have been fixed. The analyzer now reports whole-run content gaps as well as the recreation neighborhood, preventing a clean local window from hiding a different outlier.

The VMM is unchanged in this delivery. [The asynchronous block-I/O implementation plan](10-async-block-io-plan.md) describes the next fix and its persistence/reset acceptance gates. Disabling flushes, returning success before persistence, or changing the Vulkan present mode would conceal symptoms or weaken storage guarantees.

## Implementation

`BenchmarkRequest` adds two Vulkan-only, default-off options:

- `--trace-stages` records bounded per-stage timings, emits Android ATrace slices/counters, and enables the scene-frame marker. Ordinary **Check Vulkan** runs have no marker or retained stage rows.
- `--no-recreate` supplies a control run without deliberate recreation. It does not disable handling of a real out-of-date surface. The trace analyzer rejects unexpected lifecycle events in a control.

Stage records distinguish begin, acquire, draw/submit, present, fence completion and query overhead, including generation/image/frame identity and monotonic timestamps. Records are bounded to 128 rows: initial frames, the recreation neighborhood, and long outliers. Full enabled ATrace slices remain available in the system trace.

The primary Vulkan interval series now retains the interval spanning recreation and any out-of-date recovery. It no longer resets its previous measured timestamp at those events. Final Metal and Software reports verify that sample 90 equals the first recreated frame's interval, including lifecycle work. CPU draw/submission time and device timestamp queries remain separate measurements.

The Rust report explicitly tags `frame_interval_scope` as `vulkan_image_acquisition` or `guest_render_loop`; legacy reports deserialize with `unspecified`. Existing field names remain available. The guest report records the lifecycle-inclusion flag, and the runtime verifies that the diagnostic APK honored the requested recreation/stage options. The Graphics page now labels the rate as **app frames/s** and explains that Mac display smoothness is unmeasured. The matrix evaluator carries the scope into its summaries.

## Reproduction

Use a running disposable store under `.cache`, never the user's normal profile. Each capture requires a new directory under `evidence` and leaves existing evidence intact.

```sh
rtk proxy python3 tools/trace-vulkan.py --data-dir .cache/runtime-gpu-test --output evidence/new-vulkan-recreation
rtk proxy python3 tools/trace-vulkan.py --data-dir .cache/runtime-gpu-test --output evidence/new-vulkan-control --no-recreate
rtk proxy python3 tools/analyze-vulkan-trace.py evidence/new-vulkan-recreation --trace-processor .cache/trace_processor
```

The capture tool uses the guest's Perfetto service, starts its own bounded trace, and stops only the process whose command line identifies that trace output. It waits for the writer to exit, retrieves the trace in bounded chunks through GlidePod's private guest transport, then removes its guest temporary files. It does not require a public ADB port or a global tracing daemon change. The trace processor is an external local analysis tool obtained from [Perfetto's official distribution](https://get.perfetto.dev/trace_processor); it is not included in GlidePod.

## Validation and delivery

Metal (`evidence/g7/metal-validation.json`, local output) and Software (`evidence/g7/software-validation.json`, local output) pass all ten Vulkan check groups with stage markers enabled, core validation and synchronization validation, and zero errors/warnings. The packaged application also runs the ordinary unmarked validation path (`evidence/g7/packaged-validation.json`, local output). The final APK SHA-256 is `2ac2a645f6772d2a41753ba9839339127f839c90ecadb21be4a89ad16c7573cf`; source hashes (`evidence/g7/final-source-hashes.json`, local output) distinguish this fixture from the preserved G6 APK.

Delivery checks (`evidence/g7/delivery.json`, local output) record the workspace tests, formatting, strict Clippy, native build, Python checks, bundled APK identity, ad-hoc signature verification and local app process. Four older real-guest integration tests remain ignored by the ordinary workspace test command. The targeted Vulkan captures and validations above are separate real-guest runs. Native button/physical display acceptance remains unavailable; no visual inspection is claimed. Evaluation VMs are stopped after capture and the user's configuration is unchanged.

## Primary references

- [Perfetto Android system tracing](https://perfetto.dev/docs/getting-started/system-tracing).
- [Perfetto ATrace data source](https://perfetto.dev/docs/data-sources/atrace).
- [AOSP Vulkan Android swapchain implementation](https://android.googlesource.com/platform/frameworks/native/+/refs/heads/main/vulkan/libvulkan/swapchain.cpp).
