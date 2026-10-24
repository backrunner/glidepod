# Interactive UI prototype

Open [index.html](index.html) directly in a modern browser, or serve `.agents` with:

```sh
rtk proxy python3 -m http.server 4318 --bind 127.0.0.1 --directory .agents
```

Visit `http://127.0.0.1:4318/prototype/`. There is no dependency install or build step. The preview uses HTML, CSS and plain JavaScript; it is an interaction specification for the future GPUI app, not the production UI technology.

## Suggested walkthrough

1. **Apps:** the library contains user-installed apps only; Android system apps and GlidePod service components are excluded. Search for Sunday, filter Games, inspect an app using its three-dot button, then return to All apps.
2. **Desktop:** choose “Preview app windows.” Field Notes and Alpine Rally open as separate sample windows. Drag their title bars, resize from the bottom-right corner, minimize and restore from the dock, or expand with the green control. Enter a note and press Return. The game has arrow-key controls; Escape releases capture.
3. **Install:** return to the management center and choose Install APK. Use the sample APK, or choose/drop a local `.apk`. The browser only reads its name and size. Select unsupported ABI, signer mismatch or low storage to see blocking states; choose the success scenario to install Field Journal into the sample library.
4. **Finder:** use “Preview Finder install,” then double-click the sample file. The same installer opens. A single-click alternative is provided for keyboard/touch accessibility.
5. **Mac entries:** choose “Mac app entries” in the preview controls below the window. Open a sample app from the simulated application grid. Disable entries in Settings and check the unavailable state.
6. **Runtime:** open Android. Stop closes sample windows; Start brings the runtime back. Switch to Developer to see a separate sample app set. Enable root shell and confirm the restart; return to Personal to see root off and its preserved app set.
7. **Graphics:** select Software. The pending change is applied on restart. Simulate GPU failure and use either restart or software recovery in the affected game window.
8. **Widgets:** add Daylight to the desktop. Stop Android to see a paused preview. Use its Start action to resume. Native Mac widget preview explains the separate WidgetKit surface.
9. **Preferences:** try clipboard/notification/login/entry switches and light/dark appearance. Setup preview is available at the bottom of the sidebar.
10. **Reset demo:** restore the initial sample apps and preferences. This only clears this prototype's state; it cannot touch real apps or files.

## Keyboard and persistence

- `Command/Ctrl + O`: installer.
- `Command/Ctrl + K`: open the app library, focus search and select the existing query. Search matches app names and package IDs.
- `Escape`: clear a focused search, close a dialog or release game controls.
- `Tab` / `Shift + Tab`: native browser control focus; dialogs use the browser's modal focus behavior.
- `Enter`: open the first search result, or add a note in the sample notes window. IME composition confirmation does not trigger these actions.
- Preferences, mock installs, recent apps per profile and pinned widget IDs use browser `localStorage`; open windows are session-only. Each page load starts with a simulated running VM so the design can be explored immediately.

No external assets, analytics, network APIs, uploads, Android processes, host permissions or actual LaunchServices changes are used. File contents are never read. All app names, weather, game content, graphics values and storage values are fixtures. The tiny CSS game scene measures neither FPS nor Metal performance.

## Design-to-implementation mapping

| Prototype element | Intended native implementation |
| --- | --- |
| Center/sidebar/cards/inspectors | GPUI entities, typed runtime subscriptions and shared design tokens |
| Browser dialog | Native/GPUI sheet with cancellable runtime operation state |
| Simulated app article | Per-app launcher process + NSWindow + Metal content view |
| CSS Android content | Actual guest display output; no HTML app embedding |
| Desktop dock and app-entry grid | macOS Dock, Spotlight/Launchpad via registered launcher bundles |
| Widget card/panel | Android AppWidgetHost plus native desktop panel |
| Native widget preview | Separately signed SwiftUI WidgetKit extension |
| JavaScript mock state | Runtime-authoritative versioned state and durable transactions |

The subdued green palette, compact controls, restrained separators and native-style window chrome are deliberate. Central colors and spacing live in CSS custom properties for translation into GPUI tokens. Light and dark appearances and a narrow 390 px layout are provided; narrow layout is a review convenience, not an iOS product commitment. Reduced-motion preferences stop decorative animation.

## Refined controls and icon

See [UI refinement](ui-refinement.md) for the compact management layout, control behavior and icon rationale. The current glass package icon is exported from the [editable Icon Composer master](../../assets/GlidePod.icon) into [light](assets/glidepod-icon-light.png) and [dark](assets/glidepod-icon-dark.png) renditions. The sidebar, dock, menu bar and favicon use these assets and follow the prototype appearance. See the [size/background preview](assets/icon-preview.html). Older SVG artwork remains historical source.

## Screenshots from the running prototype

These are browser captures, not generated mockup images and not native runtime evidence.

Current refinement:

- [App library](screenshots/06-refined-apps.jpg)
- [Widgets](screenshots/07-refined-widgets.jpg)
- [Focused search](screenshots/08-refined-search.jpg)
- [Icon study](screenshots/09-refined-icon.jpg)
- [Dark appearance](screenshots/10-refined-dark.jpg)
- [Narrow controls](screenshots/11-refined-narrow.jpg)

The earlier `01`–`05` captures are retained as the previous design iteration, including the separate app-window and desktop-widget examples.

See [validation](../validation.md) for the completed checks and remaining limitations.
