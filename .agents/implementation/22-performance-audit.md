# Overall performance audit and improvements

Date: 2026-11-08. The largest measured host-integration bottleneck was repeated Android process startup for file and clipboard observations. A shared read-only channel removes that cost. Pixel normalization, export parsing and idle status publication also received bounded changes. The guest engine, renderer pins, memory governor and input authorization remain unchanged by this batch.

## Measurements

Apple M5 Max, 64 GiB RAM, macOS 27.2 (26B123), Rust 1.99.1. The running test guest uses four vCPUs, 4 GiB RAM, Cuttlefish 16644040, Software rendering and the existing patched Avo engine. Measurements use an isolated, newly initialized store without GMS. Baseline and candidate host-path/polling comparisons use the same guest boot, sequentially, without concurrent builds.

| Operation | Before p50 | After p50 | Before p95 | After p95 |
| --- | ---: | ---: | ---: | ---: |
| File event observation | 892.777 ms | 0.286 ms | 903.139 ms | 0.341 ms |
| Clipboard observation | 894.243 ms | 0.417 ms | 904.851 ms | 0.627 ms |
| Live 720×1080 frame read and BGRA normalization | 1.809 ms | 0.422 ms | 2.787 ms | 0.941 ms |
| Epoch authorization | 0.017 ms | 0.018 ms | 0.036 ms | 0.030 ms |
| Input heartbeat round trip | 0.349 ms | 0.309 ms | 0.444 ms | 0.447 ms |

Provider timings exclude the first connection through ten warmup iterations and retain 30 samples. Epoch and input measurements retain 300 and 180 samples respectively. Frame observations span eight seconds, separating new frames from unchanged sequence numbers. The fixture updates independently, so the number of fresh frames differs; these readings are host work, not presented FPS or physical input latency. Input still occasionally has an approximately 40 ms tail outside p95; that tail is not fixed by this batch.

Two real guest displays with the existing per-window polling cadence were measured twice per implementation. Each worker waits 350 ms after its cycle; one also reads the clipboard. Baseline VMM CPU was 50.42% and 51.90%; candidate CPU was 35.87% and 33.54%, about 32% lower at the mean. Here 100% means one occupied CPU core. The candidate completed 56 and 57 cycles per window in approximately 20.3 seconds, versus 10 and 16–17 in approximately 21.5 seconds. These measurements include Android and its software rendering but exclude GPUI windows, native clipboard synchronization and WindowServer. They do not establish a whole-desktop CPU reduction of the same percentage.

The isolated pixel conversion benchmark alternates old/new order, warms 30 iterations and measures 300 per path in the optimized development runtime:

| Pixels | Byte swap p50 | Word permutation p50 |
| --- | ---: | ---: |
| 720×1080 | 0.381 ms | 0.080 ms |
| 1080×1920 | 1.034 ms | 0.214 ms |
| 2048×2048 | 2.151 ms | 0.435 ms |

This is approximately 79–80% less time in the conversion itself. Already-BGRA frames still skip conversion. The copied-frame benchmark remains a separate synthetic path and excludes live IPC and GPUI upload.

## Implementation and failure handling

- `provider_poll.rs` and `Poll.java` share one serialized observation stream across clones of a `Store`, with exact endpoint/epoch identity and a fresh authorization check per call. Only fixed file-event and clipboard reads are admitted. Responses are live, not cached. Mutation operations retain their existing one-shot path. The connection handshake has a five-second transport limit, queries a two-second transport limit, and initial helper deployment uses the existing bridge transfer path. Any failed observation invalidates the channel without silently replaying that call. The next observation reconnects. A stale guest handle cannot invalidate a newer guest's channel. Host-side reuse expires after ten idle seconds; the Android helper exits after fifteen idle seconds and releases its external provider references.
- Pixel normalization uses safe whole-word arithmetic with explicit byte order. It preserves green, alpha and incomplete trailing bytes. There is no unsafe pixel reinterpretation, extra buffer or new graphics format.
- ADB export parsing advances a cursor across complete records and compacts only the remaining partial record once per packet. Previously it shifted the remainder of a potentially 1 MiB packet after every 64 KiB record. Fragmentation, size checks, checksums at the surrounding file layer, failure replies and transfer deadlines remain intact. No end-to-end transfer throughput percentage is attributed to this change.
- The export worker publishes and synchronizes its status snapshot only when its value changes. Failed publications remain eligible for retry. File data, metadata, receipts and durable export commits retain their existing synchronization.

The observation helper adds one resident Android process per active shared channel. Process footprint grew over the sequential workload; that is not evidence of either a memory saving or a leak. This batch does not claim lower total resident RAM. Continue using the [backing-aware memory audit](resident-memory-audit.md), which distinguishes guest backing from host/guest alias accounting and logical compressed pages.

## Graphics checks

Six GLES 3 cases per renderer cover 720×1080 with 1/16/64 objects, 1080×1920 with 16 objects, no host frame consumer, and synchronized submission. Each records 120 frames after warmup. Both Software and Metal passed the GLES 3 correctness probes, with mean guest-loop rates of approximately 60 FPS and no intervals above 33.33 ms in these short samples. At 64 objects/720×1080, whole-VMM CPU was 183.78% for Software and 82.18% for Metal. This is a current renderer comparison, not an improvement caused by changing the renderer in this batch; Automatic still selects Software.

Direct Vulkan passed on both renderers. A separate Metal run with validation enabled reported zero validation errors. Its frame interval measures successful image acquisition, including swapchain recreation, rather than native presentation. The non-validation maximum intervals were 59.83 ms for Software and 76.64 ms for Metal, with p95 values of 18.39 and 19.11 ms. The existing [BufferQueue/recreation investigation](09-vulkan-frame-pacing.md) explains why these producer intervals alone cannot establish an equivalent visible stall. No workaround was added to conceal the outlier.

## Idle resources and validation

A twenty-second sample under the packaged background owner, with no managed app windows, Software rendering, manual 4 GiB capacity and active free-page reporting, measured 10.0% VMM CPU (one-core scale). Guest RAM backing was 2418 MiB resident and 1678 MiB unbacked, with no paged-out backing in that sample. The process ledger showed 3787 MiB footprint, which is a different accounting scope and must not be added to guest backing. Android reported approximately 1425 MiB Used RAM and zero recent memory PSI. This is a short post-workload snapshot without GMS, not a long-run idle budget or a before/after memory improvement.

Across the same ten status-file observations, inode and modification time stayed unchanged. Automatic download export separately passed, including byte preservation and not recreating a deliberately deleted Mac copy. Thus the unchanged-status optimization removes idle status-file churn while retaining export behavior.

Final local checks passed:

- 104 ordinary Rust tests; 17 opt-in tests skipped by the default runner. Relevant opt-in workloads were run separately.
- Locked workspace Clippy with all targets and warnings denied, formatting and diff checks.
- Complete Android Java/DEX/APK builds, Rust workspace build, development packaging and deep/strict ad-hoc signature verification.
- Two-display input isolation, rich clipboard byte integrity, full file bridge contracts, and file failure/retry/recovery checks.
- Observation-channel reuse across Store clones, helper death, idle termination/reconnect and rejection of stale epochs.
- Both pixel microbenchmarks and the Software/Metal graphics checks above.

The file-contract fixture owns its exporter and must run with the automatic exporter absent; the initial attempt with the normal owner conflicted with that ownership and was rerun under a direct candidate worker. The ignore annotation now makes this precondition explicit. Background service verification uses the packaged helper: an unbundled debug executable cannot initialize macOS UserNotifications successfully on this host. Physical native presentation, notification interaction and long-duration GMS behavior were not qualified by these checks.

The rebuilt development bundle is `dist/GlidePod.app`. Exact source/binary hashes and check counts are recorded in `evidence/performance-20261108/validation.json`. No commit, push, installed-app replacement or release was performed.

## Remaining priorities

1. **Graphics delivery and native pacing.** The path still performs renderer readback, copied transport and GPUI texture upload. Lower CPU conversion cost does not remove these copies. Measure actual native content intervals, occlusion/restoration and sustained Metal workloads before treating a guest-loop FPS as display smoothness.
2. **Storage scheduling and capacity.** The external engine still has synchronous block operations, including flushes; the [async I/O plan](10-async-block-io-plan.md) remains relevant. This audit also found the historical GMS test store at 96% data usage and a newly initialized integration store at 88%. `/data/gsi/remount` accounted for roughly 6.6 GiB in the older store. The first measurement attempt failed an APK installation for insufficient allocatable space and was excluded. This is a userdebug remount/image-capacity issue requiring a separate migration-safe storage change; unrelated user files were not removed.
3. **Input tail latency.** Preserve the fast normal path while tracing the occasional roughly 40 ms round trip. Average/p95 improvements elsewhere do not remove this outlier.
4. **Long-running resource behavior.** Existing balloon/free-page reporting and idle compaction remain in place. Broader GMS, pressure, sleep/wake and multi-hour native-window runs are separate from this short isolated evaluation.

## Reproduction and evidence

Raw measurements and logs are under `evidence/performance-20261108/`. Earlier evidence is preserved; opt-in file/input checks now accept `GLIDEPOD_TEST_EVIDENCE` to choose a new output directory. Baseline binaries were retained under `.cache/performance-20261108/`; provider A/B comparisons disable only the new fixed-read routing in the baseline build, retaining the existing shell/content implementation. They are not comparisons against the much older Git HEAD, which predates other uncommitted product work.

```sh
GLIDEPOD_TEST_STORE="$PWD/.cache/performance-clean" \
GLIDEPOD_PERF_OUTPUT="$PWD/evidence/new-host-paths.json" \
cargo test -p glidepod-runtime --locked --test performance -- --ignored --exact host_paths --nocapture

GLIDEPOD_TEST_STORE="$PWD/.cache/performance-clean" \
GLIDEPOD_PERF_OUTPUT="$PWD/evidence/new-polling-cadence.json" \
cargo test -p glidepod-runtime --locked --test performance -- --ignored --exact polling_cadence --nocapture

cargo test -p glidepod-runtime --locked --lib display::tests::swizzle_benchmark -- --ignored --nocapture
```

Run each workload alone. The opt-in performance tests refuse to overwrite an existing report and do not assert unstable timing thresholds. Raw short-call CPU percentages include warmup/startup and can be absent when the interval is too short; the twenty-second polling experiment is the basis for the CPU comparison.
