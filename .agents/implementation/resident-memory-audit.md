# Resident memory audit — 2026-11-08

## Findings

The previous 3.8–4.1 GiB `phys_footprint` observation is valid as a macOS process ledger reading, but it is **not a measurement of unique physical RAM**. It must not be described as Android's minimum operating requirement.

A standalone Hypervisor.framework reproducer (`tools/hvf-memory-probe.c`) reserves 1 GiB, lets an ARM64 guest write it, then reads the same pages through the host mapping. Process footprint rises from about 1026 MiB to 2051 MiB while `mincore` still finds about 1024 MiB of resident backing. Removing host access with `mprotect(PROT_NONE)` drops the process ledger back to about 1027 MiB; restoring access preserves the data and backing. This demonstrates host/guest alias accounting, not a second GiB of application data. The protection manipulation is confined to the isolated reproducer and is **not** a product optimization.

Compression is a second distinction: the process ledger includes the logical size of compressed internal pages. During one run, the VMM showed about 899 MiB internal resident plus 2753 MiB compressed in its ledger. Neither that sum nor the region-only `footprint` subtotal is a unique resident-memory total. Host memory pressure varied substantially during testing; absolute RSS values across separate boots are not comparable without the backing and guest measurements.

The original 4 GiB, software-rendered Android 17 userdebug guest without GMS reported:

- About 1.34 GiB Used RAM: 980 MiB process PSS plus 387 MiB kernel memory.
- About 1 GiB file cache and 1.8 GiB free pages. These `/proc/meminfo` categories overlap with parts of `dumpsys meminfo`; do not add them to Used RAM as independent buckets.
- Largest PSS entries: system server 220 MiB, System UI 139 MiB, launcher 57 MiB. RSS sums would count shared libraries repeatedly.
- ZRAM occupied about 35 MiB physically; its approximately 3 GiB capacity was not a 3 GiB allocation.

A newly installed GMS instance reported about 1.54 GiB Used RAM and about 2 GiB file cache during initialization. GMS main/persistent/unstable processes together were about 277 MiB PSS; Play Store processes added about 183 MiB PSS in that sample. This was a signed-out installation, not an authenticated long-running FCM workload.

The automatic policy is deliberately conservative: it preserves 1280 MiB of guest-reported available memory, has a 2 GiB effective floor, and shrinks at most 128 MiB every 30 seconds. On a large host its 8 GiB boot ceiling can therefore take many minutes to contract (a theoretical 6 GiB loan would take 24 minutes). That ceiling does not reserve all physical memory, but Android can use the extra space for caches. This audit retains these service-protection margins; the new compaction releases fragmented backing without requiring a smaller guest capacity.

There is also real fragmentation overhead: Android uses 4 KiB pages, macOS uses 16 KiB pages, and the current safe reclamation objects are 64 KiB. A block containing live, balloon-owned and free subpages cannot be discarded as a whole. Kernel compaction can migrate live pages together, creating complete free extents that the existing free-page reporting path can return.

## Controlled experiments

All mutations used `.cache/adaptive-memory-test`, leaving the separate user VM untouched. Experimental mapping variants stayed in `.cache/upstream/avo-memory-audit`; the reviewed patch retains private 64 KiB backing objects.

| Experiment | Observation | Decision |
| --- | --- | --- |
| Pause/resume vCPUs | No footprint reduction | Not a memory-reclamation mechanism |
| Shared anonymous mapping | No steady footprint improvement | Do not adopt |
| 16 KiB mapping/reclamation | No convincing total footprint improvement; native allocation overhead increased | Keep 64 KiB until a backing-based comparison establishes a net benefit |
| 2 GiB boot capacity | Guest boots, Used RAM falls only about 100 MiB; availability shrinks substantially | Do not reduce the default ceiling to 2 GiB |
| Explicit clean-cache drop | Roughly 300 MiB ledger reduction in the non-GMS run; GMS result depends on initialization | Diagnostic only; no periodic cache flushing |
| Screen off | Little additional memory reduction | Not adopted as a memory policy |
| Guest balloon plus free-page reporting, GMS | Allocated backing fell from 3804 to 3375 MiB after a 1 GiB balloon target | Existing automatic governor remains useful; targets are not equal to actual savings |
| Compaction after that balloon step | Backing fell another 314 MiB | Follow up with isolated compaction measurement |
| Compaction alone after the guest settled | Backing fell 2782 → 2355 MiB, a 427 MiB reduction; command took 0.86 seconds | Implement bounded idle compaction |

For the compaction-only measurement, guest free memory changed by only about 12 MiB and file cache was essentially unchanged. `system_server` and all three GMS PIDs stayed the same; PSI remained healthy. This supports reclamation of fragmented backing rather than cache deletion or service termination. Of that reduction, resident backing decreased by 285 MiB and paged-out logical backing by 141 MiB. Backing means resident pages plus the logical size of paged-out pages; it does not mean 427 MiB of uncompressed physical DRAM was released at that instant.

## Implemented behavior

The background owner now performs one low-priority compaction after 60 seconds without managed app windows, when the existing guest pressure sample is healthy and availability is at least 1024 MiB. Attempts are at least five minutes apart across window churn, and repeated failures do not create a retry loop. Manual and automatic capacity modes both benefit from free-page reporting and this compaction. Only a VM owned by this background process is eligible.

The work runs on a separate thread with a three-second guest command limit and five-second host transport limit. A fresh epoch/window/maintenance check precedes execution. The operation writes the Linux compaction sysctl; it does not drop file caches, force-stop packages, pause Android, or disable network/GMS services. A window that appears after the final check may overlap the bounded kernel operation. This is an opportunistic optimization, not a fixed idle-RAM guarantee.

The VMM also supports an explicit `memory-residency` query using `mincore`, without touching guest contents. `glidepodctl memory` presents resident, paged-out logical and unbacked guest RAM. It excludes native VMM allocations and graphics resources, and is an approximate concurrent sample. The governor and tray do not poll this scan (about 90–135 ms for the exercised 4 GiB guest). Process footprint remains available through the existing macOS monitor and is not silently divided by two or replaced with a smaller number.

Engine builds now use patch-hash-specific directories, preserving previously registered binaries when the patch changes.

## Reproduction and evidence

```sh
rtk proxy xcrun clang -Wall -Wextra -Werror -O2 tools/hvf-memory-probe.c -framework Hypervisor -o .cache/hvf-memory-probe
rtk proxy codesign --force --sign - --entitlements macos/packaging/hypervisor.entitlements .cache/hvf-memory-probe
rtk proxy .cache/hvf-memory-probe 65536 guest
rtk proxy python3 tools/build-memory-engine.py
rtk proxy target/debug/glidepodctl --data-dir .cache/adaptive-memory-test memory
rtk proxy python3 tools/audit-memory.py --data-dir .cache/adaptive-memory-test --label baseline
```

`audit-memory.py --experiments` additionally inflates a 1 GiB balloon, compacts, drops clean caches and turns the screen off, restoring the balloon/screen afterward. Use it only with a disposable guest configured with at least 4 GiB. It is not the product policy.

Local evidence includes `evidence/memory-audit/hvf-alias-protection.jsonl`, `hvf-alias-physical.txt`, `guest-baseline.txt`, the `private`, `shared`, `private16k`, `cap2048`, and `gms4096` directories, and `compact-alone.json`. Raw records are local artifacts, not redistributed Android material.

Automatic-policy, integrity and regression verification results follow below. Native macOS banners, authenticated FCM delivery, sleep/wake, Metal memory behavior and multi-day service reliability remain separate acceptance gates.

Primary sources: [XNU task footprint accounting](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c), [Linux free-page reporting](https://docs.kernel.org/mm/free_page_reporting.html), [Linux compaction implementation](https://github.com/torvalds/linux/blob/master/mm/compaction.c). The alias result is a local reproduction, not a claim that all macOS VMMs or releases have identical accounting.

## Verification results

- Workspace tests: 95 passed, 13 opt-in tests skipped. Locked workspace Clippy with warnings denied passed. Engine report-range test passed, and the reproducible engine build matched HEAD plus the exact patch.
- Real background-owned GMS instance: the idle policy triggered exactly once, completing in 146 ms. Six status requests across 100 seconds took 7–16 ms. Guest epoch, system server and all three GMS PIDs stayed unchanged, and the retained 32 MiB random buffer kept its SHA-256. Total backing changed from 3142 to 2773 MiB over the observation window; background initialization is a confounder, so the whole difference is not attributed to compaction. Record: `evidence/memory-audit/automatic-compaction.json`; reproducible check: `tools/check-idle-compaction.py` immediately after starting a fresh resident background owner.
- Three 768 MiB allocation/free/reuse cycles with GMS installed released 678, 643 and 629 MiB of backing, independently of the process-ledger counter. Live data survived and balloon inflate/deflate completed. Record: `evidence/memory-audit/reclaim-final.json`.
- All experiments used the software renderer and an isolated guest. The production policy does not contain the experimental shared mappings, smaller mapping granules, screen-off, cache-dropping or `mprotect` operations.

- Real Android storage/permission/notification integration passed again with GMS installed and the final engine: notification updates, dismissal, privacy filtering, permission enforcement and PendingIntent display activation. This verifies the Android bridge, not remote FCM or native macOS banners. Record: `evidence/memory-audit/notifications/report.json`.

- The final UI and CLI were built into the separately identified, ad-hoc signed `.cache/adaptive-ui/GlidePod Memory Test.app`; deep signature verification passed. The isolated background owner and VM were stopped afterward. The original user runtime registration was not replaced, and no commit, push or release was performed. Build provenance: `evidence/memory-audit/final-build.json`.
