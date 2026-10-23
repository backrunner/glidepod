# Design-package validation

Date: 2026-10-23. Scope: documents and browser prototype only.

## Completed checks

| Check | Result / evidence |
| --- | --- |
| JavaScript parsing | `node --check .agents/prototype/app.js` passed |
| All six navigation pages | Browser automation confirmed the selected page and content |
| App filtering/search | Games filter returned one sample game; Sunday search returned Sunday Reader |
| Finder-style install | Double-click event on the sample APK opened the same installer flow |
| Install failure states | ABI, signer and storage scenarios disabled installation and displayed distinct errors |
| Install success | Field Journal appeared in the library and opened a separate sample window |
| Concurrent windows | Journal, Field Notes and Alpine Rally existed as independent sample windows |
| Game capture/release | Capture state enabled; Escape, returning to the center and minimizing the game released it |
| VM stop/start | Stop closed sample windows; start reached the running state |
| Profile isolation | Developer omitted the Personal-only installed sample; switching back preserved it |
| Root transition | Guest root became effective in Developer after simulated restart; Personal reported root off |
| Graphics preference | Software remained pending until restart, then became the effective renderer |
| GPU recovery | Failure view was visible; restart restored the sample game scene |
| Live widgets | Added panel appeared; stopped runtime showed paused content; panel start resumed the mock runtime |
| App entry preference | Disabled entries produced an unavailable state in the entry browser |
| Appearance/reset | Dark state applied; reset restored the initial six apps, no windows and light appearance |
| Basic accessibility | No unlabeled icon-only buttons in the initial library DOM; modal semantics and visible focus styles provided |
| Narrow layout | At 390 × 844, all six pages had no document or content horizontal overflow |
| Desktop layout | Initial browser geometry checked at 1440 × 980; visible Edge capture inspected at its normal 1912 × 827 viewport |
| Console/network | Browser logs contained no JavaScript errors; only local HTML/CSS/JS requests were observed |
| Visual evidence | Saved actual Edge screenshots under `prototype/screenshots/`; visually reviewed library, desktop, narrow and dark-widget captures |
| Documentation | Relative document links, English-language content and artifact inventory checked locally |

The initial long automation batch used fixed delays and encountered timer throttling in a background webview. A later batch exceeded that tool's 15-second limit. The relevant states were inspected and GPU recovery was rerun as a bounded individual check; these were harness timing issues, not reported as runtime failures. Background-webview screenshot capture returned “not painted,” so visual review and saved captures used Edge. A stale accessibility reference after theme rerender was refreshed before continuing.

## UI refinement checks

Follow-up on 2026-10-23. The earlier checks above describe the original prototype; this section records checks actually rerun after the visual refinement.

- JavaScript syntax passed again. All local document, stylesheet, script and image references were checked. Both icon SVG files parse as XML.
- Toolbar grid, search, clear, details, appearance and dialog-close controls use SVGs; the repeated management footer is absent. The graphical icon loads in the sidebar, dock, menu bar and favicon.
- Search matched package IDs and app names, updated its live count without losing focus, cleared through its button and Escape, and showed the empty-result state. Command-K focused search; Return opened the first match. A synthetic composing Return did not launch an app. Command-K left modal focus alone.
- Recent started empty and showed the opened app afterward. Games returned its single sample title. These are prototype state checks, not native keyboard/IME certification.
- The detail dialog opened. ABI, signer and storage installer scenarios blocked installation; success added Field Journal. Stop removed app windows, paused the widget, and the panel Start action resumed the runtime. Switching to Developer preserved profile isolation and updated the breadcrumb. Enabling guest root completed its simulated restart.
- All six pages in light and dark appearance had no document or page horizontal overflow at 390 × 844. Edge screenshots were visually inspected at the normal 1912 × 827 desktop viewport and at the narrow breakpoint; the temporary browser viewport override was reset afterward.
- The refined library, search focus/clear state, Widgets page, dark appearance, narrow controls and icon study were visually inspected and captured as `06`–`11` under `prototype/screenshots/`. The icon study includes 16, 24, 40, 64 and 96 px uses plus light/dark backgrounds.
- Browser logs contained no JavaScript errors. The observed requests were local prototype assets only.

The design remains a browser specification. No new runtime, GPU, native packaging or performance claims follow from these checks.

## What this does not establish

- No Rust/GPUI binary, VMM, Cuttlefish image, Android APK or game was built or executed.
- No hardware acceleration, zero-copy export, native input latency, frame rate, energy usage or game compatibility was measured.
- No actual macOS app bundle, Launchpad/Spotlight entry, APK file association, system service, root policy or WidgetKit extension was installed.
- Browser accessibility checks do not establish production VoiceOver/Android accessibility support.
- Source review of upstream projects is not a full security/license audit or permission to redistribute every upstream binary.
- No Git repository existed in the supplied workspace; no commit, push or release was requested or performed.

The next acceptance step is the G0 engineering experiment in the [implementation plan](research/06-implementation-plan.md), with physical Mac evidence and a reproducible source/build lock.

## Native implementation follow-up

The HTML prototype evidence above remains separate from the native implementation. The initial Rust/GPUI foundation, 23 passing tests, signed local app, HVF/Metal host probes and upstream test baseline are recorded in [the G0 implementation report](implementation/01-g0-foundation.md) and native evidence (`evidence/g0/README.md`, local output).

## Android boot and macOS 27 refinement

The subsequent [implementation record](implementation/02-android-boot-and-native-refinement.md) supersedes the initial lack of Android boot evidence. The locked official ARM64 image booted under the separately built candidate VMM, its package service responded, a real Settings frame was captured, and guest power-off produced exit code 0. Rendering was software; this is not a product-integrated runtime or guest GPU verification.

The current native management center was rebuilt and visually inspected in light/dark modes, including its 820 × 560 minimum window. Search focus, text, clear and Escape, the native file picker, APK metadata sheet, background input isolation and host checks passed. The refined icon was exported using design generation 27 in all six appearances and compiled into the signed bundle as Assets.car plus an ICNS fallback. Formatting, Clippy, all 23 Rust tests, CLI checks and signatures passed again. See `evidence/ui-refinement/` for native captures and icon renders.

APK installation, guest GPU translation, seamless Android windows, launcher integration, widgets and release qualification remain open.
