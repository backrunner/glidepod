# Initial source publication

Date: 2026-10-23. Scope: source repository and contributor documentation; not a binary release.

## License decision

Retain Apache-2.0 for original GlidePod code, documentation and artwork. It permits free and commercial use, modification and redistribution, includes an express contributor patent grant, and does not require every downstream modification to be published. Redistribution still carries the license and applicable attribution/modified-file obligations. It does not grant trademark rights. See the [official license](https://www.apache.org/licenses/LICENSE-2.0) and [application guidance](https://www.apache.org/legal/apply-license).

This fits a freely available desktop project intended to work with separately licensed Android and graphics components. The project license does not replace dependency licenses or resolve rights for a future bundled runtime. The full license remains at the root; `NOTICE` identifies the project, and third-party notices include the copyright owners and exact revision for the MoltenVK patch.

## Published scope

The repository includes the Rust workspace, original Android helpers and diagnostic workloads, pinned external build metadata, the MoltenVK patch, editable icon source, build/evaluation tools, English engineering records and historical interactive prototype.

Android system images, SDK archives, VMM/renderer binaries, user APKs, instance disks, signing material and raw local diagnostic output are ignored. Discarded icon studies remain local. The selected icon render and native management-center capture are published as README media; the latter is explicitly labeled as an early setup view.

The README presents the implemented features, runtime prerequisites and open milestones. Detailed build/runtime instructions live under `.agents/development/`. Existing references to local evidence remain identifiable as artifact paths, with no broken download links or claim that raw traces are in the repository.

## Build portability and validation

Python tools now invoke their dependencies directly, without requiring the developer's RTK shell wrapper. Packaging includes the project `NOTICE`, and the license inventory tool also writes a public, host-path-free inventory under `licenses/`.

Checks performed before publication:

- Parsed all 18 Python tools and checked local links against the staged file inventory.
- Ran `tools/validate.py`: formatting, strict Clippy, 46 passing Rust tests, APK inspection and CLI contracts, Android compilation, local packaging, host probes and deep/strict ad-hoc signature checks passed. Four live-guest tests were intentionally skipped by the default suite.
- Regenerated the target-resolved inventory for 566 Cargo dependencies. Some upstream crates omit top-level license texts; their missing entries remain explicit and need follow-up before binary redistribution.
- Checked the staged files for common credential patterns, developer-specific absolute paths, excluded binaries and runtime data. No matches were found. This is a publication hygiene check, not a full security audit.
- Preserved pinned patch bytes and upstream license texts without whitespace normalization.

No new guest benchmark, physical-device check, notarization or binary release is claimed by this publication step. Current GPU results and remaining native presentation limits remain in their individual implementation records.
