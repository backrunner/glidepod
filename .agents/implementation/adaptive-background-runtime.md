# Adaptive memory and background runtime

## Product contract

Settings exposes **Smart start / stop** (default) and **Always on**. Smart mode keeps Android online for open app windows, explicit per-app background intent, notification forwarding, installed GMS, foreground services, music and downloads. An unknown activity sample is a keep-alive, never permission to stop. Once confirmed idle, wait three minutes before graceful shutdown. App launcher bundles remain usable while Android is stopped. Always on never pauses or shuts down Android for inactivity.

Closing the last window releases the GPUI process. A separate small AppKit agent owns the VM lifetime, notification relay and menu bar fallback. While the desktop has windows, its existing menu bar monitor remains authoritative. An explicit Stop survives agent restart and suppresses automatic starts until an explicit app launch. Explicit Quit stops the background owner as well. This does not register a login item or promise operation while the Mac is asleep or logged out.

## Memory

Automatic memory chooses a boot ceiling of one quarter of physical host RAM, rounded down to GiB and bounded to 2–8 GiB. This is a ceiling, not a resident-memory reservation. Existing configurations without a memory mode retain their manual allocation. Manual size choices remain available.

The pinned external VMM gains an original virtio balloon/free-page-reporting implementation. RAM is backed by distinct 64 KiB anonymous objects, bounding mapping fragmentation to one extent per 64 KiB. For complete, aligned guest-surrendered extents, the VMM unmaps the stage-2 range, replaces its host backing with fresh anonymous zero pages, and restores the mapping before completing the report. Partial host objects remain intact. Failed host replacements restore the original mapping; an impossible remap failure terminates the VMM instead of exposing unmapped or corrupt guest memory. Guest addresses, overflow, alignment and length are checked. Reporting uses writable descriptors only; inflation payloads are bounded before allocation. The reclaim counter is cumulative traffic, not current memory savings.

The pinned Android image supplies `virtio_balloon.ko`; it is loaded only in the guest, with page reporting order 4 (64 KiB). Linux's modern PCI transport compacts absent optional queues, so reporting follows inflate/deflate directly when stats and hinting are absent. The host and guest page sizes are 16 KiB and 4 KiB respectively; reclamation never rounds into a neighboring live page. Free-page reporting also runs with a manual ceiling; automatic mode additionally changes the balloon target.

The governor samples guest availability/PSI. It returns up to 512 MiB promptly when guest pressure rises or availability falls below 768 MiB. Slow reclamation uses at most 128 MiB per 30 seconds while preserving 1280 MiB reported availability and a 2 GiB effective floor. The guest can deflate on OOM. No service is force-stopped; GMS, network, notification and media services keep their normal Android behavior. macOS pressure can request slow reclamation during foreground use. Free-page reporting works independently of balloon inflation.

## Ownership and failure handling

- One file-locked background owner per data directory; bounded private Unix socket requests.
- The agent owns the existing supervisor through its lifetime pipe. Agent death still triggers guest cleanup.
- Display lifetime holds a shared activity lock; standby must obtain exclusive activity and maintenance locks.
- Existing foreground CLI sessions remain externally owned and are never stopped by the idle policy.
- System-image maintenance explicitly pauses the background policy first.
- Epoch-bound notification events keep their existing privacy/action validation. The packaged background app has its own macOS notification permission identity.
- A launch arriving during shutdown queues a restart. Failed or incomplete activity/pressure samples cannot justify additional reclamation or idle shutdown.
- The pinned Cuttlefish `GceService` device-plumbing foreground service is the only exact component exemption. Active downloads always retain Android; pending user-app downloads do too. Pending system maintenance, such as the framework certificate-transparency list, does not pin Smart mode forever. Download activity uses a read-only Developer-image root query through an external content-provider reference.
- A failed guest or unsupported memory engine remains visible as a failure; it is not silently reported as reclaimed memory or repeatedly rebooted.

## Build and verification

`tools/build-memory-engine.py` applies `engine/patches/avo-adaptive-memory.patch` to a separate checkout of the locked Avo revision, builds and ad-hoc signs it. `--data-dir PATH` registers it only against a stopped selected runtime. Original upstream checkouts and registered engine binaries are preserved.

`tools/check-adaptive-memory.py --data-dir PATH` requires a disposable running guest. It measures macOS `phys_footprint`, exercises three 768 MiB allocation/free/reuse cycles, checks an independently retained random buffer, and verifies balloon inflation/deflation without rebooting.

`tools/check-background-runtime.py --data-dir PATH --cli PATH` checks dropped IPC clients, detached lifetime, app protection across the full idle period, graceful idle stop, reopening, and persistent manual Stop. The socket explicitly clears inherited `O_NONBLOCK` on Darwin; otherwise a request arriving just after accept can be lost.

## Validation, 2026-11-08

- Ordinary workspace suite: 93 passed, 13 opt-in tests skipped. Locked Clippy with warnings denied, Android Java/DEX build, local app build, packaging and deep ad-hoc signature verification passed.
- Patched VMM range checks passed. The builder applied the patch to a clean, separate pinned checkout and compared all source bytes against HEAD plus the patch using a temporary index. Engine and patch hashes are saved beside the executable.
- Final software-rendered guest: three 768 MiB allocation/free/reuse cycles reduced `proc_pid_rusage` physical footprint by approximately 763, 755 and 613 MiB. A retained 32 MiB random buffer kept the same SHA-256 across all cycles and 256 MiB balloon inflation/deflation. Android remained booted. Local raw record: `evidence/adaptive-memory/reclaim.json`.
- Detached agent remained about 15.0–15.1 MiB through a seven-minute lifecycle test. Explicit per-app protection retained the same VM for over 210 seconds; removing protection stopped the idle guest after approximately 180 seconds. Reopening, explicit Stop, and Stop surviving agent restart passed. One hundred connect-and-disconnect clients did not kill or replace the agent. Record: `evidence/adaptive-memory/lifecycle.json`.
- Native settings were operated in a separately identified test bundle. Always on persisted; closing the last window released the desktop process while the same agent and VM remained alive. Record: `evidence/adaptive-memory/resident.json`.
- On the final build, a Wake arriving immediately after Stop reopened Android successfully. A cold app-launch URL restarted a stopped VM and acquired a managed Android display. Records: `evidence/adaptive-memory/restart-race.json` and `evidence/adaptive-memory/cold-launch.json`. All isolated test processes were stopped afterward.
- Real Android management/notification integration passed: Unicode, redaction, updates, pagination, permission enforcement, dismissal, PendingIntent activation and opening a managed display. Record: `evidence/adaptive-memory/notifications/report.json`.
- Independent software-rendered windows and device input passed again on the final patched engine with reporting enabled. This is not a Metal validation claim.
- The native permission workflow reached the background identity, but macOS reports notifications disabled for it on this machine. Native banners/clicks and real GMS/FCM delivery remain unverified. Sleep/wake, long-duration GMS workloads and Metal reclamation remain separate acceptance gates.

See the subsequent [resident memory audit](resident-memory-audit.md): it reproduces duplicate host/guest accounting, adds backing-object diagnostics, and implements bounded idle compaction. The original footprint observations below are process-ledger readings, not unique physical RAM.

The 15 MiB number is the host agent only, not Android. In the exercised 4 GiB Android guest, reported VMM `phys_footprint` still settled around 3.8–4.1 GiB after workload churn; automatic ballooning alone did not demonstrate a lower steady-state value. macOS `footprint` region accounting was lower, but these different counters must not be substituted. This implementation establishes working reclamation and low-cost VM-off residency; it does not establish a minimum GMS-on memory footprint. Further steady-state optimization needs controlled GMS measurements rather than an assumed fixed saving.

References: [virtio 1.2 balloon](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html), [Linux page reporting](https://docs.kernel.org/mm/free_page_reporting.html), [macOS reclamation feasibility in libkrun](https://github.com/libkrun/libkrun/commit/0fd8324e984d262ee143377b4385b95f33187c32). The implementation is local evaluation code; no external binary or Android image is redistributed.
