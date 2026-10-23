# Asynchronous block I/O for guest scheduling

Date: 2026-10-23. Status: **planned, not implemented**. Evidence and scope are in [the Vulkan pacing investigation](09-vulkan-frame-pacing.md). This is a VMM scheduling/persistence change, not a Vulkan rendering workaround.

## Problem and desired behavior

The pinned external VMM processes block queue notifications on a vCPU thread. Reads, writes and flushes can therefore delay that virtual CPU from returning to Android. Host samples confirm vCPU threads waiting in file synchronization. Guest scheduling also captures a compositor runnable for about 30 ms while a kernel worker occupies its CPU.

Move disk execution to a dedicated bounded worker while preserving the existing virtio request ordering, error reporting and durability contract. A queue notification should schedule work and let guest execution resume; completion should update the used ring and signal the existing interrupt only after the operation actually finishes. No successful flush response may precede the backing-file synchronization result.

## Implementation boundaries

1. Create a pinned local VMM candidate and a reproducible source patch outside the baseline binary. Register it only in a disposable store until acceptance passes. Record source revision, patch and executable hash.
2. Give each block device a serial worker owning its disk object and active queue state. Preserve per-device order for writes, reads, flushes and completion delivery. Keep descriptor validation and write-protection checks intact.
3. Make `notify` a bounded wake/coalescing operation. A full wake channel must not block a vCPU or lose the fact that available descriptors need processing. Do not spawn a thread per request or copy an unbounded number of descriptors/buffers.
4. Do not hold device/MMIO locks during disk access, synchronization or interrupt callbacks. The worker owns its queue cursor. Publish configuration and lifecycle changes separately from disk operations.
5. Tag activation with a generation. Reset invalidates new work and suppresses stale completions before guest memory or queue state can be reused. Preserve ownership of mapped memory until every in-flight access finishes. Check reset concurrently with interrupt publication, rather than relying on a generation check made before I/O.
6. Drain accepted writes and honor flush errors on orderly shutdown. Report disk failure as virtio I/O error. A stuck host synchronization must not be converted to success or silently abandoned to improve timing; VM stop needs a visible bounded failure path and safe process/memory ownership.

## Verification

Use a controllable test disk that blocks reads/writes/flushes on explicit synchronization gates. Confirm notification returns while a flush is still blocked, and that no used-ring completion is published until the gate opens. Verify write→flush→read ordering, write-protected media, I/O failure status, bounded wake handling, reset during I/O, reset before IRQ publication and shutdown during a blocked operation. These tests exercise behavior and races rather than wall-clock speed assertions.

Then boot the disposable Android image with the candidate. Install/update/uninstall APKs, write known file payloads, flush, stop normally, restart and verify hashes. Exercise a reset with pending I/O without permitting stale queue writes or interrupts. Re-run existing VMM block/queue tests and GlidePod guest lifecycle checks.

Compare baseline and candidate with the same images, RAM, vCPUs, renderer and Vulkan APK. Repeat unprofiled no-recreation and recreation captures, including longer runs and independent simultaneous apps. Report whole-run host content gaps and guest runnable delays, not only producer FPS or a selected smooth window. Use a separate host profiling run to verify disk execution has moved off vCPU threads; do not compare profiler-instrumented FPS with an unprofiled baseline.

Acceptance requires preserved persistence/error semantics, no stale completion after reset, bounded notification behavior, real guest lifecycle success, and repeated reduction of the relevant scheduling/content-arrival stalls. Physical macOS presentation and input-to-photon latency remain separate measurements.
