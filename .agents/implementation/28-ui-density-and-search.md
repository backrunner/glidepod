# UI density and search alignment

Date: 2026-11-08. The preceding runtime, native integration and graphics work was signed and pushed as `1357944674809cfe4f1bfcd029351494e5d5d0e9` before this UI pass. GitHub verified that signature. Generated binaries, runtime data and raw evidence remain ignored.

## Changes

The management center now uses short labels and a single relevant hint instead of repeating page descriptions and implementation notes. Cards use 16 px insets, rows use 10 px vertical padding, and hints share the label edge with a 16 px line height. Rows and action groups can wrap when space is limited; segmented controls retain their intrinsic width.

| Surface | Change |
| --- | --- |
| Navigation and page headings | Remove generic subtitles; retain the selected profile in the toolbar |
| App library and running apps | Remove repeated Android-app/category captions; keep names, versions, running status and actions |
| App details | Place app identity before actions; shorten background copy; align storage and permission controls |
| Android | Keep resource notes inside their cards; remove the duplicate renderer row; retain next-start and guest-root behavior |
| Graphics | Group renderer and refresh-rate controls; retain live/next-start distinctions; remove redundant driver registration labels; expose benchmark detail on demand |
| Settings | Shorten location, network, background, built-in app, Google and notification explanations; show relevant state and retain permission, data-retention and restart consequences |
| Widgets and APK review | Remove repeated capability prose; retain the running requirement, architecture warning and signature/update behavior |
| Shared controls and app windows | Use a dark off-state switch; make hover text follow the theme; center and ellipsize the app-window error row |

Destructive confirmation dialogs remain intact. Detailed engineering limits remain in the implementation records; the graphics result still distinguishes Android rendering from Mac display FPS and game compatibility. The obsolete claim that location is unavailable was removed from app permissions.

## Search field

The border remains 34 px high. The inner input and its single-line text layout both use 30 px, and the wrapper uses explicit flex centering. The pinned `gpui-component` 0.5.1 text element derives its layout and painting height from the inherited line height. Using the same height avoids a separate short line box for the placeholder, typed text and caret. Search shortcuts, clear actions and focus handling are unchanged.

## Verification

- Final locked workspace tests: **110 passed**, zero failures, **17 opt-in tests skipped**.
- Locked workspace Clippy with all targets and `-D warnings`, workspace build, formatting and diff checks pass.
- The initial pre-commit test run had one maintenance-lock contention failure in the existing Google-apps recovery test. A runtime-suite rerun, full workspace rerun and final UI workspace run passed. No cause was established and no locking behavior was changed in this UI pass.
- Source inspection covered shared buttons, segments, switches, inputs, management pages, APK review, destructive prompts, Android window controls and menu-bar state. No Android or engine source changed in this UI pass, so the prior guest performance results were not rerun or reclassified as UI evidence.
- `dist/GlidePod-UIReview-20261108.app` is a local ad-hoc test bundle with a separate bundle identity and disposable data directory. It is not a release or global installation.
- Native inspection failed twice, including after resetting the computer-use connection, with `Sky Computer Use native pipe startup failed`. No review app process was launched. Screenshots, native search typing/clear behavior, minimum-window layout and light/dark visual acceptance are **not verified** in this pass. Source and build checks do not substitute for those checks.

Local logs and bundle verification are retained in `evidence/ui-review-20261108/`. Existing dependency future-compatibility notices remain.
