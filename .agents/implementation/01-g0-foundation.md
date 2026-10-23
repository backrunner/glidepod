# G0 foundation implementation

Date: 2026-10-23. Status: **host foundation implemented; G0 remains open**.

## Delivered behavior

The native management center is a Rust/GPUI executable, backed by actual local models rather than the HTML prototype's simulated data. It uses the approved graphical icon and forest/sage/ivory palette, compact navigation, search, resource controls and direct status text. It has no fabricated installed applications, running Android VM or GPU compatibility badge.

The workspace contains seven crates: core, protocol, macOS adapters, runtime configuration, installer preflight, GPUI UI and CLI. New source uses Apache-2.0. Cargo.lock pins the resolved dependency graph; GPUI 0.2.2 and gpui-component 0.5.1 are selected explicitly.

Implemented contracts:

- Lifecycle transitions coalesce duplicate starts and require an acknowledgement from the current VM epoch. Late acknowledgements and exits cannot resurrect a stopped VM. Requested and observed root are distinct; Personal cannot request root.
- Protocol models carry major/minor versions, image identity, default-off capabilities and VM/task/display/resize/focus generations. Length-prefixed control frames reject oversized and truncated payloads. This is not yet an authenticated transport or guest service.
- Configuration writes use an exclusive file lock, expected revision, a temporary file, fsync and atomic replacement. Invalid configurations remain intact. Personal and Developer resource preferences are isolated.
- APK preflight bounds archive size, entry count, expanded size, central-directory allocation, compression ratio, manifest size and decoded string-pool allocation; rejects unsafe entry paths, duplicate entries and symlinks; hashes the same opened archive; parses binary XML package/version/SDK metadata. It neither extracts files nor claims signature or installation success. ABI paths indicate candidacy only; they do not validate ELF or page-size compatibility.
- Image preflight checks arm64/schema/provenance fields, relative path containment, file length and SHA-256. Self-authored hashes are not publisher authentication. Verification does not make an image launchable.
- The CLI exposes host reports, isolated probes, honest runtime status, local inspections and durable settings. `start` explicitly fails because no qualified backend is registered.
- The app bundle declares the APK UTI as an alternate handler and includes a generated ICNS. The UI process has no Hypervisor entitlement; the independently signed diagnostic helper owns that entitlement. Ad-hoc signatures are checked locally.

## Measured host evidence

Machine: Apple M5 Max, 64 GiB, 18 logical CPUs, 16 KiB host page size. OS: macOS 27.0.1 (26A434). Compiler used: Rust 1.99.1; installed Swift 6.4 / Xcode SDK.

| Check | Observed result | Limit |
| --- | --- | --- |
| Original HVF probe | ARM64 `MOV X0,#42; HVC #0` at EL1; X0=42, ESR=0x5a000000 | Minimal vCPU execution, not Linux or Android boot |
| Original Metal probe | Compiled and dispatched a kernel on Apple M5 Max; all 256 words match CPU reference | Host compute/readback, not guest GLES/Vulkan |
| Workspace tests | 23 passed | Models and preflight contracts, not VM integration |
| Clippy | Workspace/all targets with `-D warnings` passed | Dependencies still report future-incompatibility notices |
| Candidate VMM tests | 153 passed, 0 failed, 2 ignored after signing; serial execution | Upstream test baseline, not complete security review or Android readiness |
| Native app | Built, packaged, launched and visually inspected | Tested on this Mac only; accessibility qualification remains open |

Evidence lives in `evidence/g0` (`evidence/g0/`, local output). The UI also imposes a 30-second deadline on the diagnostic child process. The HVF probe has an isolated watchdog and releases the vCPU, mapping and VM. Hardware checks run in a separate CLI process, so their VM ownership does not belong to the UI.

The first upstream test attempt ran an unsigned binary and produced two `HV_DENIED` failures. Signing the exact test executable with the Hypervisor entitlement and running serially produced the passing baseline. `tools/check-upstream.py` makes that requirement explicit. The ignored tests remain ignored.

## Upstream findings that change the initial research

Candidate revision: `nizwar/avo@99153258763a9ccdf5d4c6de36bd631e4dd0dded`.

1. **License scope needs clarification.** The root LICENSE is Apache-2.0, but `engine/hvf/Cargo.toml` declares BSD-3-Clause and engine documentation states that the crates use BSD-3-Clause. A matching engine license file was not found during the targeted review. The earlier assumption that the whole VMM could be treated as Apache-2.0 was too broad and has been corrected. This foundation does not copy or link the engine.
2. **Do not adopt feature spoofing.** The graphics notes/code advertise `VK_EXT_provoking_vertex` to ANGLE while removing the unsupported feature before passing it to the underlying implementation. That can change flat-varying correctness. Audit the native ASTC capability overrides as well. Passing a feature check is insufficient when the behavior is absent.
3. **Presentation currently includes readback.** The inspected renderer uses `transfer_read_iov`. IOSurface export and lower-copy presentation remain experiments.
4. **The upstream graphics recipe is not entirely source-built.** It pins gfxstream `ec3143b4ac345af198b3f5690bfc79c728128704` and Vulkan v1.4.365, but consumes a MoltenVK 1.4.0 release archive. The lock records its hash. A GlidePod source build must account for that dependency explicitly.

These are targeted source observations, not a completed audit of every unsafe boundary, device, dependency or license.

## Reproduction

Follow the repository [README](../../README.md). `tools/package-dev.py` creates `dist/GlidePod.app`; it does not install globally, notarize or publish. `tools/create-preflight-fixture.py` deterministically generates an original binary XML fixture and an ignored unsigned APK-shaped archive. The latter has no DEX and contains a dummy non-ELF library: it is only an inspection fixture, never an Android installation test.

The macOS deployment target is configured as 15.0, but only the host listed above was tested. Rust 1.90 is the declared minimum; compilation was performed with 1.99.1. The dependencies `block 0.1.6` and `proc-macro-error2 2.0.1` emit Rust future-incompatibility reports that need dependency follow-up.

## Remaining G0 work, in dependency order

1. Resolve the candidate engine's component license scope; complete the unsafe/device/reset audit or select a separately qualified alternative. Do not silently import the engine based on its root license.
2. Establish a Linux AOSP builder and pin a clean ARM64 Cuttlefish product manifest, kernel configuration, guest gfxstream and build fingerprint. No such image or builder has been supplied or produced in this workspace.
3. Build the renderer and MoltenVK from pinned sources, remove unsupported capability overrides and run correctness probes. Host Metal compute does not satisfy this gate.
4. Boot the image on HVF to package service readiness. Record boot logs and repeatable start/stop/crash results before advertising VM management.
5. Implement authenticated supervisor/guest transport and task/display control; launch two real Android activities with independent focus, resize and native window identity.
6. Validate launcher trust/indexing, Chinese composition (not just text insertion), audio and the WidgetKit distribution experiment on clean accounts.

G1–G5 remain as specified in the [implementation plan](../research/06-implementation-plan.md). APK installation, app launchers, guest root, seamless windows, guest GPU translation, games and widgets are not marked implemented by this foundation.

## Known development limitations

GPUI's current native accessibility tree does not expose the center's custom controls; VoiceOver and full keyboard traversal are open acceptance items. APK review uses an in-window overlay and still needs a complete modal focus policy. Binary XML parsing is metadata preflight rather than a replacement for Android's package verifier. There is no IPC listener, downloadable runtime catalog, privileged daemon, telemetry upload, update service or production signing pipeline yet.

## Native interaction observations

The packaged app displayed the original icon, search magnifier, focus ring and legible primary buttons. Cross-page Command-K focused Apps search. Chinese text insertion produced the correct empty search state; this is not a full Chinese IME composition test. Switching appearance rendered both palettes. Selecting Developer and 8 GiB was observed from a separate CLI process; Personal retained 4 GiB, and the test preferences were restored. The original host probes also passed when invoked from the UI.

The synthetic APK was passed through the executable's file argument and its real package/version/API/ABI/hash appeared in the review overlay. The native picker opened, but automation could not reliably complete its Go to Folder panel on this OS, so picker selection, drag/drop, Finder double-click/default association and Launchpad indexing remain unverified. No user APK was read. Screenshots are saved in `evidence/g0/screenshots/`.
