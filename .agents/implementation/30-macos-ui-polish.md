# macOS interface polish and generated-output cleanup

Date: 2026-11-10. Local development review on macOS 27.2 (26B123).

## Changes

- Centralize neutral surface, text, separator, control, hover, and accent colors. Keep green for actions and selection. The GPUI content remains opaque; this is not a claim of native Liquid Glass rendering.
- Group the sidebar into Library and Device, reduce branding, show the active profile beneath the app name, and place Android state at the bottom. Use one page heading in the toolbar. Keep Install APK available with its native picker and Command-O shortcut.
- Use consistent 13-point body/control typography, 12-point secondary labels, rounded grouped surfaces, and readable focus outlines. Wrap library filters/search and network options when constrained. Shared styling covers app details, Android resources, graphics, settings, widgets, and APK review.
- Add System appearance for new configurations. Older serialized configurations preserve their explicit Light/Dark choice. The management center follows the current system appearance without writing OS appearance changes to disk. Settings writes retain revision/conflict protection.
- Give the stopped/unconfigured library a direct start/setup action. Keep starting, stopping, failed, and missing-image status distinct. Bound and allow dismissal of footer messages. Shorten coordinate placeholders so neither is clipped.
- Replace Android-window text click targets for keyboard mode, pointer mode, and downloads with shared focusable buttons. Allocate 36 points for the control bar and preserve frame/input geometry calculations.
- Restore management focus when Escape closes APK review and avoid navigating to Settings underneath an active APK review.
- Remove eight unused embedded icon bindings and the obsolete custom shortcut-label plumbing. Keep source artwork used by historical design artifacts.

Apple references: [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass), [Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars). The implementation emphasizes readable content and clear navigation. The existing native SwiftUI popover supplies its system presentation.

## Cleanup

Measured before deleting: `.cache` 46 GiB, `target` 8.4 GiB, `dist` 490 MiB, `evidence` 704 MiB. Removed `target/debug/incremental` (approximately 2.4 GiB) and three inactive 122 MiB review bundles: UIReview, Readback, and HighRefresh dated 20261108. The active `dist/GlidePod.app`, Android images, SDKs, runtime stores, source mirrors, evidence and editable artwork were retained. Verification regenerated roughly 536 MiB of incremental data and one 123 MiB review bundle; final totals are 6.8 GiB for `target` and 247 MiB for `dist`, a net reduction of approximately 1.8 GiB after all verification outputs.

## Validation

- Locked workspace Clippy, all targets, warnings denied: passed. Upstream future-compatibility notices for `block` and `proc-macro-error2` remain.
- Locked workspace tests: **112 passed**, **17 ignored**. New behavioral tests cover legacy appearance deserialization/round-trip and explicit versus system appearance, including vibrant modes.
- Locked workspace build, formatting, and diff checks: passed.
- Packaged development app and strict deep ad-hoc signature validation: passed. No version bump, commit, push, global installation, or publication.
- Native screenshots and interaction review: Apps, Running empty state, Widgets, Android, Graphics, Settings, light/dark switching, selecting System, Command-K search/typing, Command-comma navigation, native Command-O file selection, APK metadata review, and Escape sheet dismissal. Local captures are in `evidence/ui-polish-20261110/`.
- Final settings captures verify that coordinate placeholders fit and network selections share a visible container. Captures are real native app windows, not HTML mockups. The purple window-sharing control belongs to macOS capture UI.
- Review uses an isolated empty runtime store. Live Android app controls, populated app details, install/guest behavior, VoiceOver and minimum-window visual acceptance were not verified. Native resize gestures did not change the review window dimensions; the configured 820 × 560 minimum is not counted as passed visual evidence.

The review bundle is `dist/GlidePod-UIReview-20261110.app`. It has a distinct bundle identity and a launcher setting `GLIDEPOD_DATA_DIR=/tmp/glidepod-ui-review-20261110`. An initial relaunch stalled in a filesystem `chmod` on the external-volume review directory; a process sample was retained. Moving disposable review data to `/tmp` restored launch without changing the user's store. This does not establish the cause of that filesystem stall.

The native tray source/tests and asset README changed concurrently during this pass and were left untouched by this work.

## Run the editable implementation

The Rust sources are the editable, runnable design. The historical HTML prototype remains a dated concept, not a representation of this revision.

```sh
cargo build --workspace --locked
GLIDEPOD_DATA_DIR=/tmp/glidepod-ui-review-local target/debug/glidepod
```

On this checkout, open `dist/GlidePod-UIReview-20261110.app` for the isolated packaged review. Select System, Light or Dark in Settings; use Command-K to inspect search and Command-O to review a local APK without installing it.
