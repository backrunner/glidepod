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

## Graphics diagnostics

```sh
target/debug/glidepodctl benchmark --gles 3 --objects 64 --samples 180
target/debug/glidepodctl benchmark --api vulkan --validation --samples 60
```

See [Vulkan correctness and performance](../implementation/08-native-vulkan.md) and [frame pacing](../implementation/09-vulkan-frame-pacing.md). Validation layers are diagnostic and must not be enabled in performance comparisons. Current presentation reads frames back to the host; it is not zero-copy. Producer FPS does not measure native-window latency.

The [asynchronous block I/O proposal](../implementation/10-async-block-io-plan.md) is still planned. The latest investigation distinguishes swapchain queue backpressure from a separate host/guest scheduling delay; neither should be presented as a completed performance fix.
