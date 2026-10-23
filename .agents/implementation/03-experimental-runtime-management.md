# Experimental runtime management and package slot correction

> Historical record. The missing-feature list below is superseded by [record 04](04-apps-windows-and-metal.md).
Date: 2026-10-23. Scope: local development app; no release or upstream engine redistribution.

## Delivered behavior

The management center can register a local experimental runtime manifest, start Android in the Developer profile, show observed startup and shutdown states, and stop the owned VM. The signed helper supervises the separately built Avo HVF evaluation engine using the pinned revision in `engine/upstream-lock.json`. The external AOSP ARM64 userdebug image remains outside the app bundle. The Personal profile cannot run this image, and an explicit Metal request fails rather than silently selecting software rendering.

The Android page uses compact setting rows and ordinary Start/Stop controls. It shows effective guest root access and software rendering separately from saved preferences. Configuration changes apply at the next start. Profile switching is locked while a session exists, including changes made by another CLI client. Other clients can observe or stop an existing session; closing an observing client does not stop it.

A foreground `glidepodctl start` and the GPUI app use the same helper and control protocol. Closing the owning app/CLI closes the lifetime pipe, requests Android shutdown and reaps the engine. The app is not yet a persistent background service. Restart currently consists of Stop followed by Start.

## State and process ownership

- One exclusive session lock covers verification, boot and shutdown. The engine additionally locks its writable instance disks. There is no PID-file-based kill operation.
- Runtime records and the local control socket reside in the private application data directory. The socket is mode 0600, the directory is mode 0700. Control requests and replies have bounded framing and timeouts.
- A live control response is required to report Running. A stale Running record becomes Failed after the owner disappears. A held lock with an unavailable endpoint reports preparation, never Running.
- The engine and nine required image files are rehashed before each start. This detects changed local artifacts; local hashes and a declared revision are not publisher signatures or source-build attestations. `tools/register-experiment.py` additionally checks the clean pinned source checkout and prior boot evidence.
- Writable disks are created under `developer/instance`, separate from the original boot experiment. Temporary private engine sockets use a short path for macOS Unix socket limits.
- Readiness requires a fresh complete console reply: `sys.boot_completed=1`, successful `su 0 id -u=0`, and the exact registered build fingerprint. Shell command echo and previous session output cannot acknowledge readiness. Guest root means Android access only; it is unrelated to macOS root.
- Startup has a 120-second deadline after engine spawn. Shutdown waits up to 15 seconds, then kills and reaps the owned child and reports the forced shutdown as Failed. Dropping a child guard on an error also kills and reaps it.
- Client socket option/read/write failures are isolated from VM lifetime. Real testing found that macOS may return EINVAL when a client disappears around accept; this path now closes that connection without terminating the VM.
- The pinned external engine also watches its parent and attempts shutdown if the helper disappears. That upstream fallback is source-reviewed; the current integration suite directly tests owner disappearance and engine failure, not an arbitrary helper crash before its watcher is initialized.

## Local use

Build the app with `rtk proxy python3 tools/package-dev.py`. If the pinned engine and images from the G0 experiment already exist, run `rtk proxy python3 tools/register-experiment.py`. This registers measured external paths in the normal local data directory; it does not copy the engine into the bundle or enable the Developer profile automatically.

In the app, open Android, select Developer, then Start Android. Stop Android ends the session. To import the same inventory into another data directory, choose Register Runtime and select the generated `experimental-runtime.json`; it is reverified before being saved.

For a separate CLI session:

```sh
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-session-test profile developer
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-session-test start
# From a second terminal:
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-session-test status
rtk proxy target/debug/glidepodctl --data-dir .cache/runtime-session-test stop
```

The foreground start command stays attached until stop or failure. Ctrl-C closes its ownership pipe and waits for shutdown. `--json start` produces newline-delimited state changes; ordinary one-shot JSON commands produce one JSON object. Successful output is on stdout and errors are on stderr.

## Icon correction

The front package opening now matches the original mark's relative position: 40% across and 30% down the unrotated front square, with a width of 39% of the square. The previous recreation used 48% across, 22% down and a 50% width. Its aperture, rim contours and dark insert were updated together. Both package bodies remain equal squares, centered as a pair. The selected forest green palette and restrained Icon Composer material are unchanged.

Editable sources and all appearance exports are in `.agents/design/icon-composer`. The app receives both the compiled asset catalog and ICNS fallback.

## Verification

- 29 Rust tests passed, covering configuration CAS, profile policy, image verification, stale lifecycle events, bounded console parsing and owned-child cleanup, alongside existing APK/parser tests.
- Workspace formatting and Clippy with `--all-targets --locked -- -D warnings` passed. Existing future-compatibility notices remain in third-party `block` and `proc-macro-error2` dependencies.
- `tools/check-runtime-session.py` exercised the actual external engine and image: two graceful start/stop cycles, repeated start rejection, active-profile switch rejection, rapidly disconnected control clients, owner termination, engine SIGKILL failure detection, and owner exit during startup. See `evidence/g1/runtime-session/report.json` and per-cycle JSON records.
- Ready states reported the actual Android 17 image fingerprint, Developer profile, 4 CPUs/4096 MiB and software rendering. Engine processes were observed to disappear after the completed running-session cases.
- The updated app bundle was rebuilt, signature-checked and launched. Native button interaction remains pending: desktop automation could not access windows, including Finder, and the user confirmed unlocking was unavailable and requested preserving the build. This is not counted as a UI pass. Packaged source hashes and the limitation are recorded in `evidence/g1/runtime-session/app-validation.json`.

## Remaining gates

This is an experimental management slice, not a qualified production runtime. Android GPU/Metal acceleration, APK installation, app/task inventory, per-app native windows, launchers, root toggling, live display transport and widgets remain unimplemented. The image's `su` access is intrinsic to this Developer-only adapter; the UI does not offer a misleading root-off switch. `backend_ready` remains false, and GPU/install/task-display capabilities remain false.

Next work should establish a constrained guest control channel and display transport, then APK installation/task identity management. The renderer qualification remains independent: host Metal availability or successful software boot does not validate Android GPU rendering, games, or zero-copy presentation. Source/license and distribution gates for the external engine remain open.
