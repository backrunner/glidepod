# Native UI and functional audit — 2026-10-24

This audit used the unlocked Mac, the real packaged GPUI application, the local interactive prototype, and configured Android guests. It is development evidence on this Apple Silicon/macOS host, not a release qualification or a claim that every prototype feature is implemented.

## UI comparison and changes

Reference: `.agents/prototype/index.html`, `styles.css`, `controls.css`, and `app.js`. Every management page was opened in the native application and compared with the prototype. The current editable brand icon remains authoritative. The initial comparison found an older prototype mark and an early sidebar raster; the subsequent icon integration replaces both with light/dark exports from the current Icon Composer master.

| Surface | Implemented alignment | Remaining difference |
| --- | --- | --- |
| Shared shell | 202 px sidebar, brand and subtitle, six adjacent navigation entries, profile/page breadcrumb, primary Install APK action, 22 px page titles and compact subtitles, matching green light/dark tokens; redundant sidebar runtime footer and global Mac apps action removed | The real macOS titlebar differs from the simulated desktop; prototype-only setup/demo controls are excluded |
| Apps | User-installed apps only; grouped 64 px rows, smaller icons, category/version metadata, Open/Focus and details actions, All apps/Games/Recent filters, 34 px search field in the page, counts and empty states | Categories use available Android metadata; details remain an inline inspector with real storage and permission operations |
| Running | Same grouped rows, Focus and Close window actions; independent of the Apps search/filter | No force-stop or per-window telemetry controls yet |
| Android | Same heading, grouped settings, profile and resource controls; real lifecycle and storage state | Runtime/image registration is explicit; restart and the rootless Personal runtime remain incomplete |
| Graphics | Same visual hierarchy and grouped controls; host checks and real GLES/Vulkan results | Additional experimental-renderer and honest measurement details replace illustrative game metrics |
| Widgets | Same page shell and live-panel entry; actual Android widget host opens from the page | Native WidgetKit extension and a native provider gallery remain unimplemented |
| Settings | Shared sections and switch controls, light/dark selection and persisted settings | Login/menu-bar service and automatic launcher creation shown in the prototype remain absent |
| APK installation | Review installation heading, metadata, review-before-install and status | The GPUI review is inline rather than the prototype's centered modal |
| Android app window | Theme-aware footer and Android Back action | Rotate, scale and performance overlays from the prototype are not implemented |

Search shortcuts wait for the Apps input to enter the rendered focus tree before selecting the existing query, so repeated Command-K/Command-F replaces it. Return opens the first filtered result when no IME composition is active. Recent apps now records successful opens/focus in order, persists up to 20 entries, and remains separate for each profile. Running no longer inherits the Apps page's filter. The widget host has a readable title even while the refreshed catalog is loading.

## Current icon integration

The application sidebar, prototype menu bar/sidebar/Dock/favicon, icon preview and README now use light/dark renditions exported from `assets/GlidePod.icon`. `tools/export-icons.py` regenerates the shared 256-pixel interface assets and the byte-identical self-contained prototype copies. The obsolete `.agents/media` raster was removed. The macOS bundle retains the layered `Assets.car` and ICNS fallback compiled from the same editable master.

After rebuilding and packaging, the real GPUI application was checked in both appearances, and Finder's Get Info preview confirmed the system application icon. The prototype was opened at a fresh localhost origin to bypass an older cached script; all three brand images and the favicon loaded the correct light/dark PNGs. Build, Clippy with warnings denied, JavaScript syntax, diff checks and deep/strict ad-hoc signature verification passed. Local screenshots, DOM resource checks and asset hashes are in `evidence/icon-integration-2026-10-24/`. The application was restored to light appearance with Android running. No global installation, commit, push or release was performed.

## Task lifecycle defect found during native use

Closing an app's physical display alone allowed Android to reparent its task. A later Back action in the widget window exposed an older Clock task. The runtime now removes only the matching package's task on its exclusively leased, non-default display before removing that display. It preserves package data and does not force-stop the application. The existing input heartbeat reports task completion; the native window closes after its Android task finishes. All guest operations retain VM epoch checks.

The real guest regression verifies task removal, preservation of the other independent window, and Back completion. Device input and focus-lease regressions were rerun after the bridge change.

## Pointer coordinate defect found during native use

The Android surface's transparent input canvas used absolute positioning without an explicit top/left anchor. Its computed origin followed the preceding image (y=726 px instead of the image origin), so native clicks mapped outside the guest and were discarded. Temporary instrumentation confirmed that both the NSWindow and input lease were active; the earlier automation-focus hypothesis was incorrect. The canvas is now anchored to the image's top-left corner, which also corrects the text input handler's bounds. The temporary instrumentation was removed. Native fixture tap counts advanced from 0 to 1 in touch mode and from 1 to 2 in mouse mode. Clicking the editor followed by Chinese/emoji paste also succeeded. In a second app, native clicks switched Clock to Stopwatch and started/stopped/reset its timer. These native checks supplement the existing backend input regressions.

## Executed validation

Local evidence is under `evidence/native-audit-2026-10-24/` and is intentionally ignored by Git.

- Workspace formatting, Clippy with warnings denied, build and tests; 47 ordinary tests passed. All four opt-in live tests were also exercised during this audit.
- Real native navigation across all six pages; light/dark appearance and persistence; search, query replacement and Return-to-open; APK picker, metadata review and installation; app details, cache clearing, creation of a Mac launcher and actual Finder launch while running and after a cold start; VM start/stop; host ARM64 execution and Metal compute checks; opening Clock and the widget host; the native widget picker, analog-clock binding/rendering and removal; native Back closed the widget window and the Running count returned to zero after closing Clock.
- Native resource selection saved 6 GiB / 6 vCPUs, confirmed in the configuration file, then restored to 4 GiB / 4 vCPUs.
- Real guest lifecycle: install/update, reviewed digest mismatch rejection, unsigned APK rejection, system package removal rejection, catalog/icons, guest-root policy, simultaneous displays, duplicate-window rejection, Unicode/clipboard, pointer input, widget picking/binding/restoration, uninstall/reinstall, task cleanup and Back completion.
- Real guest management: app/cache/data sizes, permission grants/revocations and Android notification provider behavior using disposable original fixtures.
- Real guest independent frame/device-input and native focus-lease workers, including stale/held input cleanup.
- Metal GLES 3: 13 targeted correctness checks passed; `gl_error=0`.
- Direct Vulkan through Metal: 10 workload checks passed; validation and synchronization validation enabled; zero errors and zero warnings. These measurements exclude native window presentation and do not establish game compatibility.
- Final development bundle was rebuilt, packaged and ad-hoc signature verified. It is not notarized or publicly published.

Key files: `workspace-tests-final.log`, `clippy-final.log`, `build-final.log`, `android-build.log`, `guest-tests.log`, `lifecycle-regression.log`, `input-regression.log`, `focus-regression.log`, `metal-gles3.json`, `metal-vulkan.json`, and the `final-*.png` native captures. Earlier screenshots preserve the installer, app details and before state. `prototype-*.png` captures are design references only.

## Evidence limits

Native text entry and clipboard paste of Chinese/emoji reached the Android editor. This does not verify a physical keyboard IME candidate/composition workflow. Backend injection and focus-worker checks are separate evidence. Physical gamepad input, audible output, macOS notification permission/banner/click/dismiss behavior, Spotlight/Launchpad indexing, older macOS versions and VoiceOver accessibility remain unqualified. GPUI currently exposes little internal accessibility structure. Guest widgets were exercised, but native WidgetKit is absent. Personal/rootless operation remains a known capability gap.

## Repository cleanup

Removed only inactive disposable VM instances and trivial generated debris, after checking their locks. Small logs and metadata were preserved in `evidence/native-audit-2026-10-24/cleanup/` with a manifest. The first cleanup reclaimed approximately **6.57 GiB of allocated disk space**:

- `.cache/runtime-session-test/developer/instance`
- `.cache/runtime-gpu-test/developer/instance`
- `.cache/gpu-evaluation-instance`
- `tools/__pycache__`
- `.agents/design/icon-composer/.DS_Store`

The lifecycle regression recreated the first disposable instance; it was removed again after its owner shut down and its session lock could be acquired. Runtime source, image originals, renderer, Android SDK/NDK/JDK, development signing key, useful build cache, editable icon/design sources and the user's live data store are preserved. No unrelated app data is removed. Temporary diagnostic instrumentation was removed before the final build. Generated artifacts remain in the existing ignored output directories, while this durable audit and the documentation index are tracked source.

Test-only fixture, secondary fixture, GLES benchmark and Vulkan APKs installed by this audit were uninstalled from the live test profile. The generated fixture Mac launcher was also removed. The product widget/input/notification helpers were retained.

Initial audit handoff: the rebuilt `dist/GlidePod.app` was open in the Apps page, Developer profile, 4 GiB / 4 vCPUs, guest root request off, and the Metal Android runtime running. The temporary localhost prototype server and browser tab were closed. No commit, push or release was performed.

## Follow-up: user-installed library and simplified navigation

The library now excludes Android system apps and GlidePod's input, widget and notification helpers. One predicate applies before Apps/Running, search, Games and Recent filtering, so visible counts and keyboard opening use the same set. The complete runtime catalog stays available for internal widget and Android settings operations. Package-name prefixes are not used to guess whether an application is user-installed.

The redundant sidebar runtime footer and global Mac apps toolbar button were removed. Runtime information stays on the Android page. The per-app launcher action remains in details as “Create Mac shortcut.” The settings SVG's offset outline was replaced with a centered eight-tooth gear, and navigation icons use an explicit shared size. The prototype uses the same gear and simplified shell; obsolete footer/button styles were removed.

The local guest initially had 12 system launchers plus the GlidePod widget helper and no user apps. The real UI displayed an empty library. A temporary original test APK was installed, and exactly one row appeared after refresh. Searching for the system Clock returned zero results; Recent showed only the test app despite stored Clock/widget history. The per-app shortcut control and light/dark settings were inspected. The fixture was then uninstalled and the library returned to zero user apps. UI tests passed (3 passed; the opt-in focus test was not repeated), as did build, Clippy with warnings denied, JavaScript/SVG checks and bundle signing verification. Screenshots and the validation record are under `evidence/library-simplification-2026-10-24/`.

Follow-up handoff: the updated development app remains open on Apps in light appearance. Android is stopped, matching this follow-up's initial state. No system packages or user data were removed; only this follow-up's disposable fixture was uninstalled. No commit, push or release was performed.


## Follow-up: measured prototype parity

The earlier visual pass did not adequately match the prototype. The native shell placed its toolbar above both columns, shifting the entire sidebar downward. The component library also overrode icon/text sizes, selected-state colors, and hover foregrounds. These were fixed at the shared control and layout level.

- The sidebar is now full height, 202 px wide at the default window size, with the wordmark at y=59, navigation at y=129, 36 px rows, 18 px icons, 11 px icon/label spacing, and the prototype's light/dark selected and hover colors. Running shows a count of visible user-installed apps with open windows.
- The workspace toolbar is 58 px high. Install APK displays Command-O and retains the actual native file picker action. Page padding is 26 px vertically and 28 px horizontally; the 22 px heading uses an explicit 26.4 px line height.
- Shared buttons own their typography and state colors while retaining the existing GPUI button event/focus handling. Segments use 28 px buttons, a 2 px inset, 7 px outer corners, and a separate selected surface. Search is 268 x 34 px, with a 15 px icon, 12 px input, Command-K hint, clear button, and 3 px focus halo.
- Settings switches use the prototype's 32 x 20 px track and 16 px white thumb. Settings cards use 18 px padding, 9 px corners and 16 px block spacing. A real-window check exposed and fixed horizontal overflow from long descriptions by allowing the workspace flex child to shrink.
- Search empty states now include Clear search; the empty Running page includes Open app library. The prototype's 980 px compact layout breakpoint is represented in the native sidebar, page padding and search width.

Evidence: `evidence/ui-parity-2026-10-24/`. Reference captures use the original prototype inside a temporary 1000 x 680 frame; only the simulated desktop frame and preview-only footer were suppressed. No internal prototype component rules were changed to improve the comparison. Native originals are 2000 x 1360 Retina captures; side-by-side images scale them to the same 1000 x 680 logical size. Platform font rasterization, real macOS titlebar controls, and prototype-only preview actions remain visible differences. The macOS purple screen-sharing indicator in native screenshots belongs to the OS, not GlidePod.

Executed this pass: six native navigation pages; light/dark appearance; All apps/Games/Recent selection; search focus/input; clearing from the field and empty state; Command-O both from the page and from a focused search field; cancellation of the native picker; audio switch on/off with its original value restored; and scrolling the Android resources/developer cards. Build, formatting, Clippy with warnings denied and signature verification passed. UI tests passed (3 passed, 1 opt-in focus test skipped). The guest integration suite was not repeated for this visual-only follow-up. Programmatic window-resize attempts did not resize the native window, so compact-width behavior has source inspection only, not a native screenshot acceptance claim.

Final handoff: the updated development bundle is open on Apps, light appearance, with the Developer profile and Android stopped. Test search text is cleared, the original audio setting is restored, and no user apps or data were changed. The temporary capture document, browser tab and localhost server were removed after recording evidence. No commit, push or release was performed.

## Follow-up: Retina icon clarity and application identity

The native sidebar previously sampled a 256-pixel PNG into an 80-pixel Retina rectangle through GPUI's bilinear atlas sampler without mipmaps. Thin contours aliased. The sidebar now selects dedicated 40/80/120-pixel exports for its 40-point display size from the current window backing scale. Both appearances are rendered directly by Icon Composer, and the prototype uses matching `srcset` density descriptors. The 256-pixel images remain documentation previews.

The two package contours now use one 4-unit bright stroke instead of a 2.4-unit core plus an 11-unit low-opacity halo. Geometry, face gradients, glass overlap and colors are preserved in both editable masters. The narrower transition improves perceived edge definition without applying raster sharpening.

The application bundle contains the current layered `Assets.car`, which already provides native renditions through 1024 pixels. Packaging now also generates a complete ICNS fallback from that catalog at 16/32/128/256/512 points in both 1x and 2x. The exporter checks that its chosen source has at least the requested physical resolution. The prior actool fallback was intentionally partial, with a maximum of 256 pixels. The app directory timestamp is refreshed after signing, and this local build was explicitly registered with Launch Services and relaunched.

Verification: UI build, Clippy with warnings denied, formatting, JavaScript syntax, three UI tests (one opt-in focus test skipped), bundle signature and icon-resolution checks passed. Native 2000 x 1360 Retina captures show the sidebar in both appearances; Finder captures show the new external application icon at normal and enlarged sizes. Evidence is in `evidence/icon-retina-2026-10-24/`, including `sidebar-comparison.png`, `finder-retina-after.png` and `validation.json`. Physical switching to 1x/3x displays and older macOS were not tested. Dock capture was unavailable from the current UI automation surface, so no Dock visual acceptance is claimed. The running development bundle remains on Apps in light appearance with Android stopped. No release, global installation or user-data changes were performed.

## Follow-up: brighter emerald icon

The user compared the icon with neighboring Dock apps and found its flat forest background and low-contrast faces too subdued. The chosen refinement retains the package geometry but uses an emerald conic background, brighter mint/ivory faces, a darker opening and reduced group shadows. The conic center is diffused under the overlap. A continuous 2048-pixel color field is generated from editable JSON parameters by a Swift tool; the five foreground layers remain vector. An initial SVG-wedge approximation was rejected after visible interference appeared in exported previews.

Both masters, all six appearance previews, native density-specific assets, documentation and prototype images were regenerated. The development UI was rebuilt and packaged with the layered catalog and complete Retina ICNS, signature checked, registered with Launch Services, and relaunched. Native captures verify the light/dark sidebar and Finder at normal and enlarged icon sizes. `evidence/icon-brightness-2026-10-24/finder-comparison.png` compares two actual Finder Retina captures at the same size; the option sheets are asset previews, not Dock screenshots. Resolution checks and source-copy equality passed. Runtime tests were not repeated for this artwork-only change. The app remains on Apps in light appearance with Android stopped. No release, global installation or user-data changes were performed.

## Follow-up: restrained center lighting

The user preferred a bright center and dark perimeter, then found the initial radial result too luminous. The final artwork uses a broader, lower-intensity emerald radial field and softer jade/ivory faces, selected from three small/large exported variants. Narrow contours and the dark opening preserve separation at sidebar and application-icon sizes. During the radial pass, the background was corrected from an untransformed 2048-pixel bitmap to the native 1024-unit canvas: Icon Composer had center-cropped the larger image, hiding the intended perimeter. The generator now checks the canvas dimension; the source still supplies the full 512-point @2x resolution.

Both editable masters, all appearance exports, Retina sidebar images and prototype copies are synchronized. The UI rebuilt successfully, and the binary was checked for all current density assets. The bundle was packaged, signature-checked, registered and relaunched; icon-resolution and master-copy checks passed. Actual Retina captures show the native sidebar and the external app icon in Finder at normal and enlarged sizes. `evidence/icon-balanced-2026-10-24/finder-comparison.png` compares two actual Finder captures at the same physical size; `candidates.png` is an exported-asset study. No new Dock visual acceptance is claimed. Runtime tests were not repeated for this artwork-only change.

Handoff: the updated development app remains on Apps in light appearance, Developer profile, with Android stopped. The Finder check window was closed and its previous icon-size setting restored. No release, global installation or user-data changes were performed.

## Source delivery verification

Before the requested source push, workspace formatting, Clippy with warnings denied, and workspace tests were rerun successfully: 48 passed and 4 opt-in guest/focus tests ignored. Those opt-in tests retain the earlier live evidence described above; they were not rerun for source delivery. The changed Android bridge compiled to Java classes and DEX successfully. Prototype JavaScript syntax, changed Python/SVG parsing and diff whitespace checks also passed. The current application bundle and Retina artwork had already been rebuilt and visually checked in the preceding artwork pass. Local VM data, credentials, build outputs and raw evidence remain ignored. This delivery is a source update, without a version bump or release publication.
