# Independent windows and device input

Date: 2026-10-23. Scope: the registered experimental Developer runtime on Apple Silicon. This record supersedes the input implementation described in record 04. It does not qualify arbitrary Android games or a production distribution.

## Window and rendering contract

Each app keeps its own native GPUI window, Android logical display, scanout, size, frame generation and texture. Different packages can be visible and update concurrently. Package and display-slot locks reject duplicate ownership. An app launch waits for Android to finish launching before returning its window session.

Each native window has a separate frame-reading worker and a latest-frame mailbox. Reading a slow display cannot block another display's reader or the input worker. New frames replace obsolete pending frames, and retired GPUI textures are released. Presentation still copies guest frame pixels into the GPUI texture atlas; this is not IOSurface sharing. The windows share the GlidePod process and Dock identity.

The supervisor now polls its management listener rather than sleeping for 40 ms after each iteration. Accepted connections are explicitly blocking with bounded timeouts, and replies are written as complete messages. This retains live epoch checks without imposing a frame/input-rate ceiling.

## Input contract

The native window owns a focus lease. Activation assigns a fresh lease immediately; deactivation invalidates it. Queued events from older leases are discarded. Before a newly active window takes Android focus, the input coordinator releases the previous window's keyboard, pointer and controller state. Focus changes and input dispatch are serialized across app windows; frame acquisition remains independent.

A persistent, private ADB stream runs the original `dev.glidepod.bridge.Input` helper. Its display, dimensions and package are fixed at launch. Keyboard and pointer events carry the target Android display explicitly. Gamepad events enter the registered virtio controller through its evdev node, preserving the Android device ID, source and motion ranges. The helper verifies that the gamepad target owns Android's focused task before reporting controller state. It uses the existing Developer guest's shell/input permissions; it does not expose a TCP listener or change device-node permissions.

The native queue is bounded at 256 actions. Adjacent motion with unchanged pointer IDs or button masks can be coalesced. Key transitions, pointer additions/removals, button transitions, wheel steps and text are not silently dropped. Overflow releases input and reports an error. The guest cancels touches and releases held keys and controller state on reset, stream shutdown or missed heartbeats. It checks for heartbeat expiry every 250 ms after 1.5 seconds of inactivity. Closing the host application also triggers the existing VM-owner shutdown contract.

Pointer-down positions remain distinct from subsequent coalesced moves, preserving Android hit testing. A held drag continues through window-level move callbacks when the pointer leaves the app surface; coordinates clamp to the guest canvas. Final window cleanup uses the same focus coordinator so closing the old input worker cannot race a new window's controller state.

The ADB stream combines a response acknowledgement with the next request in a single write. This removes a measured approximately 40 ms inter-message delay while retaining classic ADB flow control. Slow, oversized, invalid or disconnected streams fail closed. VM epochs are checked against the live owner before input is sent.

## User controls

| Control | Behavior |
| --- | --- |
| Keys: Text | Native macOS composition and committed Unicode text; navigation/delete/enter use Android key events |
| Keys: Game | Key-down, key-up, repeat and simultaneous held keys, including letters, numbers, arrows, navigation and function keys |
| Command-Shift-K | Toggle Text / Game keyboard mode; release held input first |
| Command-V / C / X / A | Paste / copy / cut / select all through the package-guarded guest IME |
| Touch | Left-button down begins a touch immediately; motion streams while held; release ends the touch |
| Option-drag in Touch mode | Two mirrored contacts for pinch gestures; transport supports up to ten explicit contacts |
| Mouse | Absolute pointer hover, left/right/middle button transitions and horizontal/vertical wheel axes |
| Command-Shift-M | Toggle Touch / Mouse mode; release held input first |
| Controller | The first compatible connected GameController extended gamepad follows the active app window; disconnection sends a neutral report |

Game-category apps initially use Game keyboard mode. Mac Command shortcuts remain host shortcuts. Keyboard mapping uses GPUI logical keys, so layout-specific physical scan-code binding is not promised. Text mode retains local macOS marked-text composition; rich surrounding-document synchronization remains open.

The GameController adapter maps face buttons, shoulders, optional Menu/Options/Home and stick buttons, two sticks, analog triggers and the directional pad. It retains one controller until it disconnects rather than merging controllers. Trigger values use Android's analog trigger axes. This version does not implement rumble, controller-specific touchpads/gyro, multiple simultaneous players, or relative mouse capture/cursor locking for FPS games.

## Acceptance and evidence

The original fixtures record events inside the receiving Android activities. Two different APK packages expose their display IDs and continuously update an on-screen counter. They consume controller events themselves, avoiding Android's default gamepad-to-navigation fallback during controller tests.

Run the following against an already-running disposable, registered test store:

```sh
rtk proxy python3 tools/build-android.py
rtk proxy env GLIDEPOD_TEST_STORE=/absolute/path/to/.cache/test-store cargo test -p glidepod-runtime --test window_input -- --ignored --nocapture
rtk proxy env GLIDEPOD_TEST_STORE=/absolute/path/to/.cache/test-store cargo test -p glidepod-ui real_focus_lease_routing -- --ignored --nocapture
```

The runtime suite checks separate frame sequences, device-specific routing, key holds/repeat/rollover/modifiers, immediate touch-down before a long hold, two-contact transitions, native mouse buttons and scroll axes, controller values/device ranges, focus reset, heartbeat expiry, close cleanup and exclusive input ownership. The host coordinator suite drives actual Rust window-input workers against both Android apps, including Unicode/clipboard after focus changes, 16 rapid focus changes, stale producers and closing an active window with held keyboard/controller input while another window takes focus. It is not a native macOS mouse/keyboard event test.

Current evidence lives under `evidence/g3/`: `metal-input`, `software-input`, `host-focus` and `bundle-validation.json`. Source-level checks include formatting, all workspace tests, strict Clippy, the native GameController snapshot mapping probe and ad-hoc bundle signature verification. Consult the JSON reports for actual results and timings; input acknowledgement timing excludes physical-device, GPUI scheduling and display scanout latency. Frame-count observations are not a game FPS benchmark.

The final local run passed 43 workspace tests (three opt-in guest tests excluded from that runner), both real renderer/input suites, the real host focus suite and the existing APK/catalog/clipboard/widget lifecycle suite in both renderer modes. The development app was rebuilt, signature-checked and relaunched from `dist/GlidePod.app`.

| Final input run | Frames observed per display in 2 seconds | Motion ACK median / p95 (60 samples) | Result |
| --- | --- | --- | --- |
| Metal (`evidence/g3/metal-input/report.json`, local output) | 43 / 45 | 0.387 / 0.507 ms | Passed |
| Software (`evidence/g3/software-input/report.json`, local output) | 42 / 41 | 0.418 / 0.509 ms | Passed |
| Host focus and close (`evidence/g3/host-focus/report.json`, local output) | Separate test; no FPS claim | Not measured | Passed |

Guest captures show display 30 (`evidence/g3/metal-input/display-0.png`, local output) and display 31 (`evidence/g3/metal-input/display-1.png`, local output), with different content and dimensions. They are frame-transport captures, not desktop screenshots. Bundle validation (`evidence/g3/bundle-validation.json`, local output) records executable/helper hashes and the launched process; cleanup (`evidence/g3/cleanup.json`, local output) records the shutdown of both disposable VMs.

Native desktop inspection still returns `cgWindowNotFound` in the locked session. Native focus callbacks, real keyboard/IME candidates, physical mouse/trackpad interaction and Bluetooth/USB controller hotplug remain manual hardware acceptance gates. The implementation does not claim that these were exercised physically. The existing GLES 2.0 limitation, rootless Personal image, separate Dock identities, dynamic guest resizing, native WidgetKit and distribution qualification remain open.

## References

- [AOSP multi-display input routing](https://source.android.com/docs/core/display/multi_display/input-routing)
- [Android controller input and device motion ranges](https://developer.android.com/games/sdk/game-controller/controller-input)
- [Android InputDevice reference](https://developer.android.com/reference/android/view/InputDevice)
- [Apple GCExtendedGamepad](https://developer.apple.com/documentation/gamecontroller/gcextendedgamepad)
- [Linux input event codes and synchronization](https://www.kernel.org/doc/html/latest/input/event-codes.html)

Private framework calls and the evdev device layout are pinned-runtime integration details, validated against the registered Android 17 image. A production guest should expose a versioned, supported bridge service rather than assume those interfaces are stable across images.
