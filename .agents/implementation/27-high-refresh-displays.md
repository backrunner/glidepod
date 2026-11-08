# 90/120 Hz display timing and frame delivery

Date: 2026-11-08. Follow-up to [record 26](26-metal-readback-tail.md).

## Scope and acceptance

The goal is to qualify 120 fps on the direct GLES Metal experiment and evaluate 90 fps as an alternative when frame delivery cannot sustain 120. Guest display mode, guest rendering, host frame delivery and physical Mac presentation are distinct measurements. A configured refresh rate alone is not a performance result.

The pinned VMM already supports `--refresh-rate HZ` for its primary display and `display INDEX WIDTH HEIGHT HZ` for independent app displays. GlidePod previously omitted both values, leaving the VMM defaults at 60 Hz. Its frame worker and benchmark also slept for approximately 16.7 ms, and GPUI notification polling added another timer phase.

## Implementation

`ProfileSettings.refresh_rate_hz` accepts 60, 90 or 120. Missing fields deserialize as 60, including historical runtime snapshots. Both profile validation and CLI configuration enforce the range. Graphics settings show the running VM's rate separately from the next-start preference. A new VM supplies the selected rate to its primary display; every app display reads the running session's settings so changing a preference cannot mix display timings within that session.

The initial 120 Hz run reached 119.97 guest frames/s over 3,600 samples, but periodic host capture delivered only 98.07 unique frames/s. The host's timer delay was therefore a separate limit. The accepted transport uses a persistent VMM frame-notification subscription. It still reads the newest validated shared-memory buffer and checks the VM epoch and file generation; events do not carry image bytes. The bounded line parser preserves fragmented events across timeouts, filters scanout identity, rejects oversized lines and disconnects, and wakes periodically for shutdown. Frame reads no longer add a fixed sleep after an arrival.

GPUI is notified through a bounded asynchronous wake channel. The existing single pending pixel slot and consumption backpressure remain. An occluded consumer does not accumulate images or wake requests. Controller and housekeeping polling use the configured cadence; renderer wakeup is driven by frame arrival. The pinned GPUI macOS backend uses a display link bound to the Mac display. The attached display reports a 144 Hz mode; this hardware inventory is not physical presentation measurement.

The benchmark records configured rate, guest-observed rate, guest render-loop intervals, and intervals between successful unique host captures. Host capture spans the entire invocation, including warmup and completion/provider overhead; it must not be compared as though it were the guest's exact measured interval. GLES sample capacity increases to 3,600 so a 120 Hz run can cover approximately 30 measured seconds after warmup.

## Measurement protocol

Use the same direct GLES artifact (`3a6dad2877b0a1e5`), experimental VMM (`0875af3ec1b6` patch-set directory), MoltenVK for native Vulkan, four vCPUs, manual 4 GiB RAM, 720×1080 scene, 16 objects, `--finish-each-frame` and host capture. Run 3,600 samples per pass. Disable validation/tracing and finish builds before measurement. Preserve all results on the shared development host; no process belonging to unrelated work is stopped.

Evidence is retained in `evidence/high-refresh-20261108/`. The first timer-based run is retained as diagnostic evidence separately from the event-driven comparison.

## Reproduction

Register and launch the direct GLES candidate as described in record 25, selecting the desired rate before starting:

```sh
rtk proxy target/debug/glidepodctl --data-dir .cache/your-disposable-store configure --renderer metal --refresh-rate 120
# Launch the registered experiment with the ANGLE environment from record 25.
rtk proxy target/debug/glidepodctl --data-dir .cache/your-disposable-store --json benchmark --gles 3 --samples 3600 --finish-each-frame
```

Restart Android after selecting 90 or 60 Hz. Actual application rates may be lower because of rendering cost or application frame-rate policy. Android's [frame-rate API](https://developer.android.com/media/optimize/performance/frame-rate) and [multiple-refresh-rate policy](https://source.android.com/docs/core/graphics/multiple-refresh-rate) describe the distinction between supported modes, application requests and selected rates. The implementation does not override arbitrary games' pacing policies.

## High refresh results

Each pass has 3,600 measured guest frames after two seconds of warmup. All feature/pixel checks pass. These are individual runs, not pooled percentiles:

| Rate | Pass | Guest frames/s | Unique host captures/s | Guest interval p99 ms |
| --- | --- | ---: | ---: | ---: |
| 120 Hz | 1 | 115.31 | 115.51 | 21.99 |
| 120 Hz | 2 | 119.86 | 119.82 | 11.07 |
| 90 Hz | 1 | 90.00 | 90.02 | 13.39 |
| 90 Hz | 2 | 89.99 | 90.00 | 13.79 |
| 60 Hz | baseline | 57.69 | 57.77 | 35.54 |

The later 60 Hz baseline passes correctness and delivers host frames at the guest rate, but contains a 439.66 ms maximum guest pause. It is retained rather than replaced by a faster run. This shared-host session is not a clean isolated benchmark, and the data alone does not establish the cause of each pause.

120 Hz is attainable in this scene, including host delivery. The first pass has lower throughput and longer tails, so it is not an unconditional sustained-120 guarantee. Both 90 Hz passes sustain the target with similar tail latency; this is the conservative option for less consistent workloads. A 90 Hz frame budget is 11.11 ms and a 120 Hz budget is 8.33 ms, so neither reported p99 means every frame meets its budget. The Mac's shared workload and different graphics APIs/games remain qualification variables.

## Compatibility and functional checks

Workspace tests and Clippy cover the new configuration and stream parser. The parser regression tests a split notification across a timeout, another display's events, oversized input and EOF. An initial test writer deadlocked on the macOS socket buffer before the reader ran; the test was corrected to stream oversized input from a concurrent writer. The failed attempt is retained in the evidence log.

At 120 Hz, Vulkan passes its ten checks with validation enabled, zero errors and zero warnings. The mixed-display check receives 841 Vulkan frames and 630 GLES frames on separate displays while the overlay remains unchanged. All twelve independent-input checks pass. A next-start isolation check changes the stored preference to 90 while the VM is still running at 120, then verifies a new benchmark display remains at 120.

A separately signed test bundle pins `GLIDEPOD_DATA_DIR` to the disposable store. Native launch and the Apps page render successfully. Repeated CUA clicks did not navigate to Graphics, so the new settings page and animated native app-window acceptance remain incomplete. No physical Mac presentation-rate claim follows from the guest/host capture results. The isolated native UI and its background helper are closed before subsequent performance runs.

## Delivery and restoration

The local test bundle is `dist/GlidePod-HighRefresh-20261108.app`, with a distinct test bundle identity and `LSEnvironment` pointing to the disposable store. Its ad-hoc signature, executable UUIDs and fixture hashes are verified. It is not a global installation. No commit, push or publication was requested.

All five final display runs pass correctness. The final workspace run has 110 passed tests, zero failures and 17 opt-in skips; the relevant ignored input test is run separately and passes. Final locked workspace Clippy (`--all-targets -- -D warnings`), Android compilation, formatting and diff checks pass. Existing dependency future-compatibility and Android deprecation notices remain.

The disposable VM is stopped, its original engine is restored exactly, and its settings are returned to Software and 60 Hz. MoltenVK remains selected and both Vulkan driver registrations are retained. The user's separate configuration and runtime/GPU registration files remain byte-identical. The native test UI and background helper are no longer running. Machine-readable results, provenance and restoration checks are in `comparison.json`, `validation.json` and `package-verification.json` under the evidence directory.
