# Experimental Android runtime

The source checkout builds the management app and its Android helpers. To run Android applications, prepare and register an external VMM and Cuttlefish ARM64 image. This is a developer workflow; there is no production runtime installer yet.

## Prepare the software-rendered baseline

After [building GlidePod](building.md), run from the repository root:

```sh
python3 tools/check-upstream.py
python3 tools/fetch-cuttlefish.py
python3 tools/boot-experiment.py
python3 tools/register-experiment.py
```

The first command checks out the pinned Avo candidate into `.cache/upstream/avo`, compiles its VMM tests, signs the test executables for Hypervisor.framework and runs them serially. The image download is about 1.1 GB; allow additional space for extracted images, per-instance disks and toolchains. The boot experiment uses four vCPUs, 4 GiB RAM and software rendering, captures an Android frame, requests power-off and writes a local report. Registration uses that report and the measured external artifact paths.

In GlidePod, select **Developer** on the Android page and start the VM. **Personal cannot boot with this userdebug adapter.** A root preference does not remove guest root access; a separately qualified rootless image is still required.

The external runtime stops when its owning app or foreground CLI exits. Closing the management center keeps the owner alive while app windows remain. To run from the terminal:

```sh
target/debug/glidepodctl profile developer
target/debug/glidepodctl start
```

`start` stays in the foreground. Ctrl-C shuts it down; `glidepodctl stop` requests shutdown from another terminal. Successful `--json` output goes to stdout, errors to stderr.

Preferences normally live in `~/Library/Application Support/GlidePod`. The CLI's `--data-dir` or `GLIDEPOD_DATA_DIR` selects an isolated store. Use a disposable directory under `.cache/` for tests; registration and profile selection must target that same store.

## Select the experimental Metal renderer

**Automatic currently selects Software.** The Metal candidate must be built and registered explicitly. It uses a source-built MoltenVK 1.4.2 with a dynamic triangle-fan patch; the earlier renderer is not sufficient for the GLES 3 checks.

```sh
bash .cache/upstream/avo/scripts/build-gfxstream.sh
python3 tools/build-metal-renderer.py
target/debug/glidepodctl register-gpu /absolute/path/printed/by/the/build
target/debug/glidepodctl configure --renderer metal
```

The renderer build also needs CMake and Ninja. Read the [GLES 3 renderer record](../implementation/07-gles3-metal.md) and [source lock](../../engine/metal-renderer-lock.json) before building. The lock currently includes hashes of the evaluated base binaries; a fresh toolchain may produce different hashes and be rejected. Do not bypass that check or claim a new build is qualified without reviewing its provenance and re-running the correctness checks. A portable source-to-binary release pipeline remains open.

This candidate enables MoltenVK's optional private Metal implementation. It is an experimental external component, not an App Store-compatible or production-qualified renderer. No Vulkan/GLES conformance or broad game compatibility is claimed.

### Evaluate KosmicKrisp on Apple Silicon

On macOS 26 or later, the same gfxstream backend can use the pinned KosmicKrisp driver from LunarG's SDK. Extract SDK **1.4.363.1** into a project-local directory using its documented `copy_only=1` installation option; a global SDK installation is unnecessary. This candidate uses an SDK binary, not a GlidePod source build. The preparation tool verifies driver/base-library hashes and creates an immutable directory:

```sh
rtk proxy python3 tools/prepare-kosmickrisp-renderer.py --sdk /absolute/path/to/extracted/sdk
rtk proxy target/debug/glidepodctl --data-dir .cache/graphics-test register-gpu /absolute/path/printed/by/the/tool --driver kosmickrisp
rtk proxy target/debug/glidepodctl --data-dir .cache/graphics-test configure --renderer metal
```

Use an already prepared disposable store and stop it before registration. The driver is pinned in `experimental-gpu.json`; the worker sets the selected ICD explicitly, so a shell's `ANDROID_EMU_VK_ICD` cannot silently substitute a different driver. Omitting `--driver` retains MoltenVK, including for older registrations. The version 2 registry retains both driver registrations and the selected driver in one atomic file; legacy registrations migrate without changing their selection. Automatic still uses Software.

In **Graphics → Vulkan driver**, register each driver directory and use **Use** to select a registered driver while Android is stopped. **Currently using** describes the live runtime; **Next Metal start** describes the saved selection. Both registration and selection verify pinned artifacts. A failed verification leaves the previous selection intact. The equivalent CLI commands are:

```sh
rtk proxy target/debug/glidepodctl gpu-driver
rtk proxy target/debug/glidepodctl gpu-driver kosmickrisp
rtk proxy target/debug/glidepodctl gpu-driver moltenvk
```

### Uncapped graphics pressure tests

Use an explicitly selected disposable project-local store. Start its intended engine and driver first, then run workloads sequentially without concurrent builds or other GPU benchmarks:

```sh
rtk proxy python3 tools/evaluate-gles-stress.py --data-dir .cache/graphics-test --output evidence/stress-moltenvk-gles --label moltenvk
rtk proxy python3 tools/evaluate-gles-stress.py --data-dir .cache/graphics-test --output evidence/stress-moltenvk-vulkan --label moltenvk --api vulkan
rtk proxy target/debug/glidepodctl --data-dir .cache/graphics-test --json vulkan-stress --objects 256 --seconds 1 --validation
```

GLES uses the same original C++ workload on Android and in the optional host ANGLE probe (`tools/gles-stress.cpp`). Vulkan uses direct Android libvulkan with an indexed, textured scene. Each waits for completion per frame; neither includes swap/presentation/readback in its timed loop. They report completed frames per second, **not display FPS**, with two seconds of warmup, 30 seconds per measured case, and two passes by default. Validation-enabled Vulkan runs must remain separate from performance samples.

The runner retains per-run identities, correctness, image probes, CPU usage and latency distributions in a new evidence directory. Diagnostic reports are also saved in `developer/stress/`. A failed correctness check returns failure even when diagnostic timings exist. `--allow-incompatible-probe` continues collecting these diagnostics but still exits with failure. See [the pressure-test and direct Metal record](../implementation/24-graphics-pressure-and-driver-settings.md) before interpreting results or using the experimental host GLES patch.

See the [graphics audit](../implementation/23-graphics-drivers-and-metal.md), [candidate lock](../../engine/kosmickrisp-renderer-lock.json), and [LunarG installation instructions](https://vulkan.lunarg.com/doc/view/latest/mac/getting_started.html). Host-driver conformance does not qualify the guest translation stack, game compatibility or redistribution of the combined runtime.

## Work with apps

With a registered runtime running:

```sh
target/debug/glidepodctl install /path/to/app.apk
target/debug/glidepodctl apps
target/debug/glidepodctl app-info dev.example.app
target/debug/glidepodctl add-launcher dev.example.app
target/debug/glidepodctl storage
target/debug/glidepodctl clear-storage dev.example.app
target/debug/glidepodctl permission dev.example.app android.permission.POST_NOTIFICATIONS revoke
```

The management center provides APK review, app launching, per-app storage and permissions, and opt-in notification forwarding. APK inspection does not verify a signer or certify compatibility. Use APKs you have permission to run; Google Play services and proprietary translation layers are not included.

Each app window has Text/Game keyboard modes and Touch/Mouse pointer modes. `⌘⇧K` and `⌘⇧M` switch them; Option-drag in Touch mode supplies two mirrored contacts. One compatible extended gamepad follows the active window, and focus changes release held input. Relative pointer capture, multiple players and rumble remain open. Physical device and native presentation validation are separate from guest event tests.

## Network diagnostics

```sh
target/debug/glidepodctl network
target/debug/glidepodctl --json network
```

This reads macOS HTTP/HTTPS/SOCKS and PAC configuration without starting a VM or changing network settings. It omits proxy credentials and PAC URLs. It does not test reachability or evaluate PAC. The pinned VMM currently uses host sockets and does **not** apply system proxy preferences; host VPN/TUN coverage depends on the actual routing policy. See the [network implementation plan](../implementation/13-vm-network-and-proxy-plan.md) for TCP, UDP, DNS, strict proxy mode and resource limits. Proxy forwarding is not implemented yet.

## Google services

Google service prerequisites in a running guest can be inspected with `target/debug/glidepodctl --data-dir PATH gms` (add `--json` before `gms` for structured output). Package presence is not a sign-in, FCM or certification result. The [Google services record](../implementation/14-google-services-and-notifications.md) describes the optional-install experiment and native notification requirements.

## Graphics diagnostics

```sh
target/debug/glidepodctl benchmark --gles 3 --objects 64 --samples 180
target/debug/glidepodctl benchmark --api vulkan --validation --samples 60
```

See [Vulkan correctness and performance](../implementation/08-native-vulkan.md) and [frame pacing](../implementation/09-vulkan-frame-pacing.md). Validation layers are diagnostic and must not be enabled in performance comparisons. Current presentation reads frames back to the host; it is not zero-copy. Producer FPS does not measure native-window latency.

The [asynchronous block I/O proposal](../implementation/10-async-block-io-plan.md) is still planned. The latest investigation distinguishes swapchain queue backpressure from a separate host/guest scheduling delay; neither should be presented as a completed performance fix.

## Optional Google services

Settings → Google services provides installation, removal, cancellation and interrupted-transaction recovery for the Android 17 ARM64 Developer runtime. CLI: `glidepodctl google-services install|remove|recover|status`. Existing Android data is preserved through candidate-disk promotion; see [transaction and notification behavior](../implementation/15-google-services-product-and-system-notifications.md). The installation record is bound to the image inventory; restore that runtime before booting or removing an installation after an incompatible image change.

Settings → Notifications forwards all entries Android exposes to its notification listener, including system, ongoing and grouped notifications. Private/secret entries keep their place with redacted content. macOS permission is required. Neither Google API availability nor remote FCM is inferred from this local relay.

## Source-built direct GLES Metal experiment

The direct GLES candidate has a separate pinned ANGLE source build and VMM patch. It currently exposes ES 3.0. Build with `rtk proxy python3 tools/build-direct-gles-metal.py` and `rtk proxy python3 tools/build-memory-engine.py --host-services --gles-metal`; use only a stopped disposable store for registration and launch. The ANGLE builder prints the immutable library directory and required launch environment. It does not change user settings or register a driver. This GLES path remains independent of the Vulkan-driver selector. Follow [record 25](../implementation/25-direct-gles-metal-correctness.md) for the patches, exact tests and launch procedure, and [record 26](../implementation/26-metal-readback-tail.md) for bounded readback, packing regressions, multi-window lifecycle fixes and the subsequent controlled measurements.

### High refresh display checks

Graphics supports 60, 90 and 120 Hz as a next-start preference. The CLI equivalent is `configure --refresh-rate 120`. Existing profiles default to 60 Hz. The primary display and independent app displays use the running VM snapshot, so restart Android to apply a new rate. Frame delivery uses bounded VMM notifications and a single pending UI frame. See [record 27](../implementation/27-high-refresh-displays.md) for measured guest rates, host capture rates and native-presentation boundaries.
