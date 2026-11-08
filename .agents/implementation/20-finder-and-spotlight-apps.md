# Installed apps in Finder and Spotlight

Installed apps now receive Mac entries before their first launch. The automatic selection uses the same `AndroidApp::in_library` policy as the desktop library: user-installed launchable apps and Google Play, excluding internal helpers. Previously opened or explicitly exported system apps retain their entries while still present in the full catalog.

## Discovery and lifecycle

- Real signed bundles live in `~/Applications/GlidePod Apps`. A filesystem link at `/Applications/GlidePod Apps` makes the folder visible through Finder's normal Applications sidebar. An occupied or protected Applications path is preserved and reported as a synchronization warning; it does not require administrator escalation or stop an app window from opening.
- Filenames use the Android display label, so Finder and Spotlight show useful names rather than package IDs. Unsafe/hidden path components are normalized, Unicode names are preserved, and name collisions receive a stable package-derived suffix. Existing package-named entries migrate on reconciliation.
- The native adapter calls the public [Launch Services registration API](https://developer.apple.com/documentation/coreservices/1446350-lsregisterurl) and requests Spotlight import with `mdimport -i`. Registration and indexing are distinct operations. No global index reset, indexing-preference change or synthetic search record is used.
- A successful live catalog refresh creates and updates entries and removes entries whose packages disappeared. A cached catalog can restore entries while Android is stopped, but never removes entries. Guest-query failure never becomes an empty authoritative catalog.
- Reconciliation only prunes managed bundles from the same Android data directory. Other stores, unrelated bundles, and symlinks are preserved. A library-wide lock serializes writes; live desktop catalog refreshes also serialize the guest snapshot with reconciliation.
- GUI APK installation/removal triggers catalog reconciliation. The CLI installation and removal commands include a `mac_launchers` report in their JSON results; `glidepodctl sync-launchers` explicitly reconciles an already running guest. The details action now says **Show in Finder**.
- Finder launches use the current guest label for the native window, including an app installed through another client since the management center loaded its catalog.

## Validation, 2026-11-08

Workspace Clippy passed with warnings denied. The workspace suite passed 84 tests, with 12 environment-dependent tests ignored. The macOS launcher tests cover same-name apps, Unicode/path sanitization, migration and rename, failed update preservation, store ownership, cached versus live pruning, and occupied Applications links. The development bundle was rebuilt and its strict ad-hoc signature verified.

Live checks used the existing disposable `host-files-20261107` Android session:

1. Reconciliation produced five entries without launching the three installed user apps. Finder's Applications folder visibly contained **GlidePod Apps**, and opening it showed the app labels and Android icons.
2. `mdls` reported application bundle identifiers and readable display names. `mdfind` returned all five entries, with no old package-named duplicates.
3. A newly built test-only package, `dev.glidepod.launcheracceptance`, was installed through the packaged CLI. Its Mac entry and Spotlight record appeared before its first launch.
4. Updating that package's label to **GlidePod Search Proof** replaced the old entry. Spotlight returned exactly one entry with the new name. A query by display name and application content type also returned it.
5. Opening the new entry through Finder launched the real Android fixture. Repeating after quitting the desktop verified cold startup and the corrected native window title.
6. The test window was closed, then the newly created test package was uninstalled. Its bundle and Spotlight search result disappeared; the five pre-existing entries remained.

Local JSON evidence is under `evidence/spotlight-20261108/`: `sync.json`, `index.json`, `install.json`, `rename.json`, `name-search.json`, and `uninstall.json`. The temporary acceptance package was removed. Finder's UI and launch path were observed directly. The Spotlight app itself could not be opened by the computer-use interface, so search evidence comes from the real Spotlight metadata index and queries rather than a captured Spotlight palette.

No commit, push, notarization or public release was performed.
