# Building GlidePod

## Requirements

- An Apple Silicon Mac and a full Xcode 27 installation, selected as the active developer directory. Packaging uses its Icon Composer asset compiler.
- Rust and Cargo, including `rustfmt` and `clippy`. Development has been validated with Rust 1.99.1; the declared Rust 1.90 minimum is not yet verified.
- Python 3.12 or newer, Git and the command-line tools supplied by Xcode.
- Internet access and sufficient free disk space for Rust dependencies, Android compile tools and build outputs. An Android runtime is a separate download.

The configured deployment target is macOS 15. Current runtime evidence comes from macOS 27.0.1 on an Apple M5 Max; this does not establish compatibility with older macOS releases. RTK is not required to build or run the project. Some historical records retain the wrapper used during development.

## Build the application

From a source checkout:

```sh
python3 tools/prepare-android-tools.py
cargo fetch --locked --target aarch64-apple-darwin
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tools/license-inventory.py
python3 tools/package-dev.py
open dist/GlidePod.app
```

`prepare-android-tools.py` downloads pinned JDK, Android SDK/NDK and optional Vulkan validation-layer artifacts into `.cache/android-tools/`, checks their recorded digests and records provenance. The downloaded tools retain their upstream terms; they are not included in this repository. Digest checks do not authenticate a publisher's signature.

`package-dev.py` builds the original Android helpers and Rust workspace, compiles the [layered icon](../../assets/README.md), creates `dist/GlidePod.app`, and verifies its ad-hoc signature. The diagnostic helper receives the Hypervisor entitlement. This is a local development bundle, not a notarized public download. No VM engine, renderer or Android system image is bundled. The management center can be explored before setting up a runtime; launching Android apps requires the [external runtime setup](runtime.md).

The app declares `.apk` support as an alternate file handler. It does not change your existing default application. `⌘O` opens the installer, `⌘F` or `⌘K` focuses search, and `⌘R` refreshes the app catalog and preferences.

## Validate a change

For host checks, original APK metadata fixtures, packaging, CLI contracts, Hypervisor/Metal probes and bundle signatures:

```sh
python3 tools/validate.py
```

Reports are written under the ignored `evidence/g0/` directory. This runner does not boot an Android guest, exercise physical input, test notification banners or establish game compatibility. The default Rust test suite skips tests that require an explicitly configured live guest.

For guest lifecycle, independent windows/input, storage/permissions and graphics, use the commands and scope in the [implementation records](../README.md). Run performance workloads sequentially with no concurrent build or competing guest, and retain the exact source, image and renderer revisions.

## Repository map

| Path | Responsibility |
| --- | --- |
| `crates/glidepod-core` | Lifecycle state, resource validation and desired/effective root state |
| `crates/glidepod-protocol` | Versioned capabilities, bounded messages and window leases |
| `crates/glidepod-runtime` | VM supervision, private ADB transport, apps, displays, input and management |
| `crates/glidepod-installer` | Bounded APK ZIP and binary XML inspection |
| `crates/glidepod-macos` | Hypervisor/Metal probes, controllers and native notifications |
| `crates/glidepod-ui` | GPUI management center and application windows |
| `crates/glidepodctl` | Diagnostics, configuration and runtime CLI |
| `android/` | Original guest helpers, diagnostic workloads and test fixtures |
| `engine/` | External dependency pins and the reviewed MoltenVK patch |
| `assets/` | Original icons and editable Icon Composer source |
| `.agents/` | Research, interactive design prototype and implementation records |
| `tools/` | Build, packaging and evaluation scripts |

Generated outputs belong in `target/`, `.cache/`, `dist/` or `evidence/`. They are not part of the source distribution.
