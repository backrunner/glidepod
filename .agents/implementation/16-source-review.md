# Source review and delivery checks

Date: 2026-11-07. Scope: the complete pending dependency, frame transport, proxy inventory, optional Google services, Android notification, UI, CLI and documentation changes since `41fb9f3`. The review includes their interactions with runtime ownership, input/display leases and the existing management workflows.

## Corrections from review

- A real post-removal restart exposed a startup race: a status reader could briefly hold the session lock and be mistaken for the new supervisor. Startup now requires an acknowledgment from that exact child through its private stdout pipe. Bounded lock retries tolerate short-lived status readers while preserving exclusion of another runtime owner; a regression test reproduces the misleading lock holder.

- Native notification responses now carry the content version actually delivered to macOS. A delayed click or dismissal for an older version cannot act on its replacement just because the Android key stayed the same. Android also rechecks package and post time before cancellation, closing the race between host synchronization and guest updates.
- Pending notification clicks reopen the management center when it has closed while another Android window keeps the app running. Previously only incoming URLs/APKs could wake it, leaving notification clicks queued indefinitely.
- Google services maintenance tolerates the original runtime control socket closing after its UI owner requests shutdown. The session lock, followed by the disk lock, remains the authority for proceeding with disk preparation.
- Recovery and image-compatibility failures are written to the normal runtime failure status. Unknown transaction formats remain intact and are not silently discarded.
- Recovery removes interrupted partial downloads and extracted payloads while preserving the fixed-name retry archive, which is checked again before use.
- A delayed removal confirmation cannot start maintenance over another UI operation that began while the prompt was open.

## Validation

Local evidence is under `evidence/review-2026-11-07/`. These ignored artifacts are not included in the source repository.

- Final host validation: formatting, locked workspace Clippy with `-D warnings`, 65 ordinary Rust tests, original Android helper/fixture builds, workspace build, packaging, ad hoc signatures, CLI contracts, Hypervisor execution and Metal compute/readback probes. Six opt-in tests remain ignored in the ordinary test run.
- Four Python archive-safety tests pass. Changed Python and JSON files parse. Regenerating the dependency inventory and license text produces identical content.
- The real Android management suite passes with GMS installed, including the new stale-dismissal regression, system/framework notifications, private/secret redaction, ongoing/group entries, pagination, PendingIntent activation and an independent notification display. This is Android-side evidence, not macOS banner acceptance.
- The final packaged helper completes a real GMS install/remove round trip from an existing disposable instance. Installation overlaps the previous CLI owner's shutdown; removal stops a running VM itself. All three Google packages are present after installation and absent after removal, and the original user-data sentinel survives every boot. Transaction, candidate, backup and installer-cache directories are cleaned, and the VM finishes stopped.
- The local cached VMM executable had been removed. It was rebuilt from the unchanged, clean Avo revision `99153258763a9ccdf5d4c6de36bd631e4dd0dded`, ad hoc signed and registered only in the disposable test stores. Its new binary digest is recorded with the test registration; Android image and renderer pins remain unchanged.

The prior [product validation](15-google-services-product-and-system-notifications.md) retains the real cancellation and process-interruption recovery evidence. The frame and graphics evidence remains in the [upstream audit](12-upstream-and-frame-performance.md); this review does not turn those host microbenchmarks into game FPS or total memory claims.

## Delivery boundaries

This is a source delivery, not a notarized release or Google certification. The user explicitly deferred native Settings and notification banner/click/dismiss acceptance. Google account login, remote FCM delivery, Play Protect/Integrity and transparent proxying of all VM traffic remain unverified or unimplemented as documented in the corresponding workstreams. Inline notification reply/media controls and arbitrary cross-package task migration inside an existing app window are not qualified by these tests.

The repository has no checked-in GitHub Actions workflow. Local validation must not be described as a remote CI result. Generated APKs, runtime binaries, guest disks, downloaded Google packages and raw evidence are excluded from the source commit.
