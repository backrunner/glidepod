# Apps, independent displays and experimental Metal

Date: 2026-10-23. This record supersedes the missing-feature list in record 03. It describes implemented code and measured guest behavior, not a completed production release. [Record 05](05-independent-windows-and-input.md) supersedes its original input path with independent workers, focus leases and continuous device input.

[Record 06](06-3d-storage-and-notifications.md) adds per-app storage/runtime permissions, opt-in notification forwarding and measured 3D workloads. It supersedes the notification gap below; native banner/callback acceptance and advanced notification actions remain open.

[Record 07](07-gles3-metal.md) adds the source-built MoltenVK 1.4.2 candidate and a dynamic triangle-fan fix. It verifies GLES 3.1 context creation and targeted correctness; the original GLES 2 observations below describe the earlier 1.4.0 renderer.

## Delivered

| Area | Implementation | Evidence and limits |
| --- | --- | --- |
| APK installation | Private staged copy, reviewed SHA-256 check, bounded transfer, Android Package Manager install/update, error reporting and temporary-file cleanup | Real signed APK install/update passed; changed reviewed bytes and unsigned APKs were rejected. Updates retain Android data. Split APK sets are not supported. |
| App library | Android labels, package IDs, launch activities, versions, icons, game category, search, updated ordering, refresh and offline catalog | Catalog comes from Package Manager through an original Java helper. Personal does not display the Developer catalog. System app removal is rejected. |
| App windows | One independent Android display per package, a GPUI window and frame/input workers per display, duplicate-window coalescing, continuous display-targeted input | Two simultaneous displays, different sizes, actual pixels and independent keyboard/touch/mouse/controller input passed with Software and Metal; see record 05. Windows currently share the GlidePod process and Dock identity. |
| Text and clipboard | Original Android IME; UTF-8 commits, copy, cut and select-all through a shell-authorized receiver; macOS composition handler and Command shortcuts | Chinese, emoji, percent characters and selection/copy round-trip passed in the guest. Physical macOS IME candidate-window interaction remains unverified while the desktop is locked. Clipboard access is action-driven, not continuous synchronization. |
| Display transport | Versioned triple-buffer frame files, bounded dimensions, generation/sequence checks, BGRA conversion, bounded queues and explicit GPUI texture retirement | Copied readback presentation. This is not shared IOSurface or zero-copy rendering. Guest canvas size remains fixed while the Mac window letterboxes it. |
| Mac entries | Signed per-user `.app` entries under `~/Applications/GlidePod Apps`, real Android icons, a small Swift launcher and `glidepod://launch/<package>` routing | Entry creation and code signature passed. Native launching/indexing in Finder, Spotlight or Launchpad has not been clicked or observed. APK association remains an alternate handler rather than replacing the user's default. |
| Runtime lifecycle | App-wide runtime ownership, reopenable management center, existing start/stop/crash cleanup and guest epoch checks | Closing the center preserves the owner while other app windows remain. Quitting the process closes its lifetime pipe. CLI lifecycle regression passed again; native window-close behavior is compiled but awaits desktop testing. |
| Developer root | Explicit CLI `--root` shortcut preference and verified guest uid | This is a convenience gate, not a root-removal boundary: a Developer shell can invoke the image's intrinsic `su`. Personal/rootless image support and a real root-off mode remain open. |
| Widgets | Original Android AppWidgetHost APK, provider picker, Android binding/configuration, live RemoteViews, remove and persisted IDs | Analog clock binding and restoration produced real frames. Provider package visibility is declared so RemoteViews can resolve resources. This is a widget panel, not a WidgetKit extension or desktop-layer integration. |
| Android GPU | Explicit registration and hash verification of external gfxstream, Vulkan loader and MoltenVK artifacts; Metal preference selects the GPU on next start | Android boot and the complete app-session test passed on Apple M5 Max. SurfaceFlinger reports ANGLE on Apple, gfxstream, Vulkan 1.4.323 and **OpenGL ES 2.0**. No fake provoking-vertex extension or GLES 3.x promotion was added. |
| Audio | Settings switch controls the external VMM's output at next boot | Existing settings retain their previous muted behavior; fresh settings enable output. Physical playback, latency and device changes are not qualified. Microphone and camera remain disabled. |

The management UI uses compact rows and direct actions. There are no simulated installed apps, decorative performance claims or fabricated game compatibility results. The approved layered main icon is retained.

## Control and ownership

The supervisor exposes its private transport paths only through a live session status response. Each guest operation checks the current running epoch. The VMM's temporary socket directory is private and unique per session. A direct ADB transport connects through Unix/vsock port 6520; it creates no host TCP listener and requires no installed `adb` executable. The current Cuttlefish development image accepts that private connection without an ADB authentication challenge. Images requiring ADB keys are explicitly rejected rather than disabling their authentication.

Install operations lock the installer, copy at most 2 GiB into a private `.apk`, inspect that exact snapshot, compare the reviewed digest when supplied, push to a bounded guest staging path, and let Android enforce signature and platform policy. A preflight report still does not claim signature verification.

App displays are leased with per-package and per-slot host locks. The runtime disables Android's default physical-display mirroring before creating displays. SurfaceFlinger physical port IDs are matched to Android logical display IDs. Input uses the actual logical display, not whichever desktop window happens to be selected in the management list. Text commits additionally require the IME's focused editor package to match the intended package. The receiver requires the Android `DUMP` sender permission, excluding ordinary app senders.

The first app open installs and enables GlidePod Input inside the Developer guest. This changes the guest's selected input method. It does not install a macOS input source. The macOS composition buffer is local; surrounding-document and rich selection synchronization are not implemented.

Closing a window releases its display. Android may keep its task/process alive in the guest; the Running page currently represents open GlidePod windows, not an authoritative inventory of all Android services and processes.

## Build and run

```sh
rtk proxy python3 tools/prepare-android-tools.py
rtk proxy python3 tools/package-dev.py
```

The toolchain preparation script downloads pinned Temurin and Android SDK build tools into ignored `.cache/android-tools`, verifies upstream digests and records measured SHA-256 values. The SDK and JDK are development inputs and are not included in the app. The package includes the original bridge JAR, input APK and widget APK, plus the notification and benchmark APKs added in record 06. APK helpers currently use a local development signing key; a stable release helper key and upgrade policy must precede distribution.

After registering the external VMM and images as described in record 03, select Developer and start Android. Add an APK, review it, then Install. App rows provide Open, Close, Add to Mac and Remove. The Widgets page installs the host and opens its panel.

```sh
rtk proxy target/debug/glidepodctl apps
rtk proxy target/debug/glidepodctl install /path/to/app.apk
rtk proxy target/debug/glidepodctl uninstall dev.example.app
rtk proxy target/debug/glidepodctl add-launcher dev.example.app
rtk proxy target/debug/glidepodctl shell 'getprop ro.build.fingerprint'
```

The CLI root shortcut must first be enabled on the Android page. It affects guest commands only and does not request host administrator access.

To evaluate the independently built GPU artifacts:

```sh
rtk proxy bash .cache/upstream/avo/scripts/build-gfxstream.sh
rtk proxy target/debug/glidepodctl register-gpu .cache/upstream/avo/build/gfxstream
rtk proxy target/debug/glidepodctl configure --renderer metal
```

Registration requires Android to be stopped and hashes the required artifacts. Automatic deliberately continues to use Software. Metal is explicit and experimental; missing or modified libraries fail before boot. The evaluated local renderer has also been registered in the development app's default store, without changing the active Personal profile or renderer preference.

## Reproducible verification

Start a disposable registered Developer VM, then run:

```sh
rtk proxy env GLIDEPOD_TEST_STORE=/absolute/project/.cache/runtime-session-test cargo test -p glidepod-runtime --test guest_session -- --ignored --nocapture
rtk proxy python3 tools/check-runtime-session.py --data-dir .cache/runtime-session-test
rtk proxy python3 tools/validate.py
```

`guest_session` intentionally refuses a store outside this project's `.cache`. It installs/removes only the original fixtures and widget helper in that disposable guest. It exercises update, unsigned rejection, reviewed-digest rejection, system removal rejection, root shortcut policy, two displays, duplicate-window exclusion, Unicode input and clipboard, pointer-driven changes, widget selection/binding/restoration captures, uninstall and reinstall.

- Software acceptance (`evidence/g2/software-session/app-session.json`, local output) and Metal acceptance (`evidence/g2/metal-session/app-session.json`, local output).
- Metal input frame (`evidence/g2/metal-session/input-after.png`, local output), second app display (`evidence/g2/metal-session/second-display.png`, local output), and restored live widget (`evidence/g2/software-session/widget-restored.png`, local output).
- GPU boot report (`evidence/g2/gpu/report.json`, local output) and its console/engine logs. Renderer strings were inspected directly; they are not inferred from a Mac capability probe.
- Launcher bundle check (`evidence/g2/launcher.json`, local output). The temporary test entry was removed afterward.
- Lifecycle regression (`evidence/g1/runtime-session/report.json`, local output) and packaged host validation (`evidence/g0/validation.json`, local output).
- Final development bundle (`evidence/g2/bundle-validation.json`, local output) records the rebuilt executable hashes, signature check and relaunched process. Cleanup (`evidence/g2/cleanup.json`, local output) confirms the evaluation VMs were stopped.

The frame captures come from the actual guest frame transport, not macOS screenshots. Native computer use still returned `cgWindowNotFound`; therefore native buttons, window resizing/focus, physical IME composition, file double-click routing and launcher indexing are not recorded as UI passes.

## Remaining work and concrete blockers

1. **Game qualification and graphics conformance.** Record 07 implements the missing provoking-vertex behavior through MoltenVK and fixes dynamic fan rendering. Its targeted GLES 3 checks pass. [Record 08](08-native-vulkan.md) adds direct Vulkan 1.3 pixel/compute/synchronization tests, independent display evidence and a one-minute synthetic load. Full graphics conformance, ASTC correctness, long thermal endurance and representative game tests remain. Controller mapping and two-contact gestures are now implemented in record 05; physical controller qualification, relative mouse capture, rumble, multiple players and low-latency audio remain.
2. **Complete fusion integration.** Per-app windows currently share the host process/Dock identity; task-aware multi-window apps, dynamic guest resize/rotation, per-app processes, crash recovery, advanced notification actions and host file sharing remain. Basic notification forwarding is implemented in record 06. Native UI acceptance requires an operable desktop.
3. **Personal isolation and root-off.** The registered image is userdebug with intrinsic root access. A verified rootless image/provisioning flow is necessary; changing a UI Boolean cannot remove `su`.
4. **Native Mac widgets.** The live Android host is implemented. A separate WidgetKit extension, snapshot/timeline bridge, signing/installation and desktop placement remain.
5. **Redistributable runtime.** The external engine and graphics libraries remain outside GlidePod.app. Resolve the upstream license declaration discrepancy, complete third-party notices, stabilize helper signing, reproduce artifacts, qualify clean-machine setup, then sign/notarize a distributable runtime. Current hashes prove local consistency, not publisher trust or legal clearance.

These gates remain explicit. `backend_ready` is still false for production qualification; running-session capability flags now reflect the implemented experimental install/display/widget/GPU paths. `shared_surfaces` remains false.
