# UI refinement

Date: 2026-10-23. Scope: the interactive browser prototype and its original icon artwork.

## Direction

Use the management center as a utility. Page titles name their content: Apps, Running apps, Android, Graphics, Widgets and Settings. Remove promotional headlines, uppercase taglines, the library hero, and the repeated bottom status/slogan bar. Keep runtime state beside the active profile in the sidebar. The outer desktop retains one unobtrusive “Interactive prototype” label.

The green palette stays. Content uses quieter separators, smaller corner radii and a consistent system font. Primary buttons identify the next operation; ordinary actions use bordered or plain controls. The app library uses rows with an icon, name, category, version or running status, Open/Focus and app details. No card lift or decorative hover motion.

## Controls

- **Search:** a real search input, SVG magnifier, 34 px control height, 12 px text, focus ring, clear action and keyboard hint. Match app names and package IDs without changing the entered text. Show a live result count and a useful empty state. Command/Ctrl-K opens the library and selects the query; Escape clears it; Return opens the first result. Composition confirmation does not open an app. Keep input focus while updating results.
- **Filters:** compact segmented buttons with explicit pressed state. Recent reflects apps actually opened in the active profile, most recent first; it is no longer a slice of the installed list. Profiles keep separate recent lists.
- **Toolbar:** replace the font-based grid glyph with a four-pane SVG and “Mac apps” label. Install APK uses the same icon stroke system. Narrow layouts retain the labeled accessible name when hiding the visible secondary label.
- **Icon buttons:** use SVG for search, clear, details, dialog close, appearance, Android Back and rotate. Give each icon-only action an accessible name. Standard control hover, pressed, disabled and keyboard-focus states are visible in both appearances.
- **Widgets:** a compact app identity, actual sample content, panel state, and a right-aligned Add/Remove action. Keep the runtime preference in one settings row. Explain cached native Mac widgets next to their preview; retain the deeper WidgetKit distinction in the implementation documents and preview dialog.
- **Dialogs:** direct operation titles such as Install APK, Review installation, Installation complete and Mac app entries. Mock-data notices remain where needed to prevent confusion with a real runtime.

## Icon

The original lettermark is replaced by two offset rounded window panes. A sage rear pane and ivory foreground pane sit on the existing forest-green tile. The foreground title-bar stroke communicates a window without a letter or platform mascot. The artwork uses solid fills, with no glow, texture, generated imagery or dependency on fonts.

- Master: [glidepod-icon.svg](assets/glidepod-icon.svg)
- Monochrome symbol: [glidepod-symbol.svg](assets/glidepod-symbol.svg)
- Size and background study: [icon-preview.html](assets/icon-preview.html)
- Colors: forest `#315E4B`, sage `#A2BAA7`, ivory `#F3F5EB`

The source is scalable vector artwork integrated in the sidebar, dock, menu bar and favicon. This is the design master, not a packaged `.icns` or final macOS release asset catalog. Native implementation must validate optical sizes and template-image treatment in the real menu bar.

## Files and native implementation

`controls.css` contains the refinement layer; `styles.css` retains the desktop and fictional guest scenes. `app.js` owns prototype interactions. The native GPUI implementation should translate these dimensions and states into shared control components, using AppKit hooks where required for text input, focus, accessibility and host integration. Browser focus testing does not establish native IME or VoiceOver correctness.
