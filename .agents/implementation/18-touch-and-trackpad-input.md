# Touch accuracy, game input and macOS trackpad gestures

Date: 2026-11-07. Scope: the native app-window input path and the registered Android 17 Developer image. This extends [the window/input contract](05-independent-windows-and-input.md). Existing file and clipboard work in the checkout was preserved.

## Supported controls

| Input | Current behavior |
| --- | --- |
| Game keyboard mode | Key down/up, repeats, simultaneous keys and modifiers. Game-category apps select this mode initially; Command-Shift-K toggles it. |
| Touch mode | Click, hold and drag; Option-drag supplies two mirrored contacts. |
| Trackpad two-finger scrolling in Touch mode | A continuous Android touch swipe, in either axis. AppKit's natural-scroll preference is retained. Android owns fling inertia; macOS momentum is consumed to avoid a second scroll. |
| Trackpad pinch and rotation | Two stable Android pointer IDs, shared across overlapping pinch/rotation phases. Both pointer modes support these gestures. |
| Mouse mode | Absolute hover, button transitions and wheel axes, including AppKit precise scrolling and momentum. Command-Shift-M toggles it. |
| Controller | The existing GameController adapter follows the active window and releases state on disconnect/focus transfer. |

Raw game keys require the Android game to support keyboard input. There is no configurable key-to-screen/virtual-joystick mapping or relative mouse capture for FPS camera control. Controller input requires game support. No particular commercial game was tested. macOS three/four-finger system gestures are outside this adapter.

## Implementation and failure handling

`glidepod-macos/src/gestures.m` installs a local AppKit event monitor for each Android window. It uses the existing NSView, only captures gestures beginning over the painted Android surface in the active key window, and continues delivery through end/cancel outside that surface. Ordinary mouse wheels and Mouse-mode scrolling remain on the GPUI path. The monitor never observes other applications, requests global input permissions, or replaces GPUI's view class.

GPUI 0.2.2 exposes scrolling but does not preserve the momentum/cancellation information needed here or expose magnification/rotation. The native monitor retains those distinctions. Its bounded 256-event queue is drained by the existing UI timer; overflow cancels the current touch gesture. The Rust handle is confined to the UI thread and removes the monitor when dropped.

`touch_gestures.rs` maps logical Mac points through the actual painted surface. Retina backing scale is not applied a second time. Tests cover letterboxing, fractional sizing, clamping and zero-size surfaces. The surface and footer do not shrink independently, and file-error banners are included in the available-height calculation. Touch release now sends the final mouse-up position before releasing contacts, so the last drag position cannot be lost when no corresponding move callback was delivered.

Pinch uses an initial span of 60% of the shorter guest dimension. The registered 720 × 1080 display reports a minimum scaling span of 340 pixels and touch slop of 18 pixels. Smaller initial spans passed transport checks but failed actual Android recognition, especially for inward pinches. Near the surface edge the transform center is inset to leave room for both contacts and expansion; ordinary taps remain at the exact mapped position. Pinch/rotation contacts remain bounded, with no independent clipping that would distort their angle. Rotation converts AppKit's counterclockwise angle into Android's downward-y coordinates.

Window deactivation, mode switches and closure retain the existing focus-lease reset. Geometry changes and direct mouse presses cancel active gestures. `cancel_touch` sends Android ACTION_CANCEL without releasing a concurrently held keyboard key or controller. Stale scroll cancellation cannot cancel a newer pinch. A stationary scroll begin/end does not manufacture a tap.

## Validation

Checks at the end of the input implementation, before the submission review below:

- Formatting and strict workspace Clippy passed; the workspace build passed. Cargo reports existing future-compatibility notices for third-party `block` and `proc-macro-error2`.
- The local `dist/GlidePod.app` was rebuilt and ad-hoc signature verification passed. Packaged executable sections and Mach-O UUID match the current build; the bridge hash also matches. Whole executable hashes differ because packaging signs the app. The test VM shut down cleanly. No commit, push, global installation or release was performed.
- Workspace tests: 78 passed, 12 opt-in tests skipped by the default runner. Three input-related opt-in tests were then run explicitly and passed.
- `tools/check-gestures-native.py`: passed in-process AppKit adapter checks for coordinates, phases, routing, window isolation, momentum, mode switching and overflow. It uses synthetic event objects and posts no desktop input.
- `independent_render_and_device_input`: passed on the real Metal guest, including held/repeated/overlapping keys, modifiers, multi-touch, mouse buttons/wheels, controller buttons/axes/device identity, watchdog, reset and stream closure.
- `real_focus_lease_routing`: passed against two actual Android applications, including rapid focus transfers, stale producers, Unicode/edit routing and closure cleanup.
- `gestures_reach_android_and_hit_targets`: three host layouts, including fractional scaling and letterboxing, clicked the same real Android button. The receiving Activity verified scroll release coordinates, pointer rotation, display identity and cancellation without releasing a held game key. Android `ScaleGestureDetector` reported outward scaling of 1.25, inward scaling of approximately 0.941, and edge scaling of approximately 1.20 after recognition began. These are recognizer results, not the full physical gesture ratio.

Local reports and guest event traces are under `evidence/input-gestures-20261107/`, including copies of the Metal input and host focus reports. Reproduce against an already running disposable registered store:

```sh
python3 tools/build-android.py
python3 tools/check-gestures-native.py
GLIDEPOD_TEST_STORE=/absolute/path/to/.cache/test-store cargo test -p glidepod-runtime --test window_input --locked -- --ignored --nocapture
GLIDEPOD_TEST_STORE=/absolute/path/to/.cache/test-store cargo test -p glidepod-ui --locked real_focus_lease_routing -- --ignored --nocapture
GLIDEPOD_TEST_STORE=/absolute/path/to/.cache/test-store cargo test -p glidepod-ui --locked gestures_reach_android_and_hit_targets -- --ignored --nocapture
```

Native desktop inspection returned `cgWindowNotFound`. Physical trackpad recognition, perceived gesture latency, actual window resize/focus callbacks, real keyboard/IME and hardware controller behavior were not accepted in this run. The guest evidence does not replace that desktop acceptance. A stale test store first rejected an engine-hash mismatch; validation used an already registered matching test store without changing the engine registration or skipping its integrity check.

## Submission review, 2026-11-07

The complete pending file/clipboard/input batch was reviewed together. The two file-lifecycle fixes and final checks are recorded in [the file integration review](17-host-files-and-clipboard-plan.md#submission-review-2026-11-07). The final workspace suite has 79 passing ordinary tests, with 12 opt-in tests skipped by default. Eight relevant guest tests were executed separately and passed, including all three input tests above against the registered software-rendered guest. The native gesture adapter test also passed again. Android helper compilation, strict Clippy, workspace build, development packaging and signature verification passed. Source submission does not extend the physical-device acceptance boundary stated above.

## References

- [Apple: Handling Trackpad Events](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html) — gesture phases, cancellation and separate scroll momentum.
- [Apple: NSEvent](https://developer.apple.com/documentation/appkit/nsevent) — local event monitors, magnification and rotation.
