# Contributing to GlidePod

GlidePod is an experimental macOS project. Small, reproducible fixes are welcome. For a new runtime backend, graphics transport or other substantial change, open an issue first so the design and validation requirements can be discussed.

## Get started

Follow the [build guide](.agents/development/building.md). The [architecture](.agents/research/02-architecture.md) describes the intended boundaries; the [implementation records](.agents/README.md) distinguish completed work from proposals. All documentation is in English.

Before submitting a change, run the checks relevant to it:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Compile Android helpers when changing guest code. For packaging or native integration changes, also build the development app and inspect the affected behavior on a Mac. The build guide explains the broader validation runner. Guest tests marked `ignored` need an explicitly prepared disposable store; they do not run in the default test suite.

## Describe the behavior and evidence

Include the problem, the resulting behavior, how you checked it, and any remaining limits. Report the Mac model, macOS version, guest build, renderer and workload when discussing graphics or input. A host Metal probe is not an Android GPU test; a guest frame capture is not a native-window presentation measurement. Keep synthetic benchmarks separate from game compatibility claims.

Preserve per-window input ownership, bounded queues and messages, explicit permission checks, and orderly VM shutdown. Storage changes must retain error and durability semantics. Never improve a benchmark by silently dropping work or changing what it measures.

Keep Android images, SDK downloads, app data, credentials, raw traces and generated binaries out of commits. Use the ignored `.cache/`, `target/`, `dist/` and `evidence/` directories. Review diagnostic output before attaching it to a public issue.

## License and dependencies

Unless explicitly stated otherwise, contributions intentionally submitted for inclusion are provided under Apache-2.0, as described in section 5 of [LICENSE](LICENSE). Only submit material you have the right to contribute. Preserve upstream copyright and license notices, identify modified third-party files, and document new dependencies in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Do not add Google services, proprietary translation libraries, third-party APKs or runtime binaries without establishing their redistribution rights. Generated license inventories are useful evidence, not a substitute for reviewing the components of an actual release.
