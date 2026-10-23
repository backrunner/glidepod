# Implementation plan and acceptance

## Sequencing

The schedule below is a planning range for a small team with virtualization/graphics, Rust/macOS, and Android-platform experience. It is not a delivery commitment. Some UI and guest work can overlap after G0; duration is not simply the sum of rows. Unknown VMM and GPU issues can change the plan substantially.

| Gate | Planning range | Concrete deliverable | Exit criteria |
| --- | --- | --- | --- |
| G0 — feasibility and provenance | 2–3 weeks | Pinned source-built VMM/renderer experiment; clean image; minimal two-window host | ARM64 boot, actual Metal rendering, two focused apps, reproducible build, license inventory, no feature spoofing; capture evidence on real Mac |
| G1 — reliable runtime | 3–5 weeks | Supervisor, image provisioner, profile storage, typed control channel, CLI | Repeated start/stop/reboot, no orphan VMM, authenticated IPC, package transaction recovery, data-preserving crash handling |
| G2 — seamless applications | 4–6 weeks | GPUI center, per-app windows and launchers, APK association, input/IME | Finder install to Mac-entry launch; multiple tasks, app switching, permissions/dialogs, resize, Chinese input, audio, close-vs-quit behavior |
| G3 — GPU and games | 4–8 weeks | Qualified graphics profile, diagnostics, controller/mouse mapping, P1 export experiment | Versioned GLES/Vulkan matrix, latency/power evidence, failure recovery, supported-game list; software mode labeled accurately |
| G4 — integration and root | 3–5 weeks | File/clipboard/notification bridges, Developer profile/root shell, live widgets | Permission enforcement, profile isolation, root desired/effective state, widget configuration/rebind, idle policy |
| G5 — distributable beta | 2–4 weeks | Signed/notarized build, reproducible images, update/rollback, native widget if qualified | Clean-account install, helper/entry repair, corresponding sources/SBOM, recovery and upgrade tests, published known limits |

G3 begins during G1; graphics feasibility is never deferred until after UI completion. Native WidgetKit packaging is investigated during G0/G1 so signing constraints are known early. Per-app `su`, advanced split archives, concurrent VMs, suspend-to-disk and broad peripheral emulation are subsequent milestones.

## G0 experiment checklist

1. Resolve exact upstream SHAs and record source provenance. Audit the candidate VMM's unsafe boundaries, guest-memory arithmetic, Virtio queues and reset behavior before reusing code.
2. Build the selected Cuttlefish ARM64 image on Linux and host libraries on Mac. Pin guest/host gfxstream compatibility; log all compiler and SDK versions.
3. Boot to package service readiness through HVF; prove vCPU hardware execution and runtime feature checks. Capture kernel/boot logs, no reliance on a developer's Android Studio installation.
4. Probe real Metal device and Vulkan/GLES capabilities; render correctness scenes. Reject unsupported features instead of adding misleading feature bits.
5. Launch two test activities on separate displays, including a non-resizable activity and a cross-app/system-dialog case. Route pointer and keyboard only to the right task.
6. Open two native windows through a prototype Rust host. Prove independent app identity using separate local shim processes, then test generated bundle trust under a clean user account.
7. Measure readback, presentation, memory and input latency. Exercise the IOSurface/GPU-copy export API on one frame before planning zero-copy optimization.
8. Compile/discover a minimal Developer ID-distributed WidgetKit extension with the chosen App Group setup. Treat failure as a scope decision, not evidence that a browser panel is a native widget.

Produce `evidence/g0/<date-machine>/` containing dependency lock, commands, logs, probe JSON, screenshots/video, traces, licenses and a pass/fail report. Do not commit user APK binaries or personal data. A prebuilt borrowed demo without corresponding build provenance does not pass.

## Proposed repository layout

```text
Cargo.toml
crates/
  glidepod-core/          # State, identifiers, durable models, error taxonomy
  glidepod-protocol/      # Versioned wire schema and capability model
  glidepod-runtime/       # Supervisor, sessions, registry, policy
  glidepod-ui/            # GPUI management center
  glidepod-window/        # App window host and presentation
  glidepod-macos/         # XPC, AppKit, Metal and OS adapters
  glidepod-installer/     # Bounded package inspection and transactions
  glidepodctl/            # Quiet CLI and JSON mode
engine/                  # Audited VMM fork with upstream patch queue
graphics/                # Pinned gfxstream/MoltenVK build + export patches
android/
  device/glidepod/        # Cuttlefish product overlay, init, SELinux
  bridge/                # System service and guest protocol
  ime/                   # Host composition bridge
  widgets/               # AppWidgetHost
macos/
  launcher/              # Per-app bundle template
  widget-extension/      # SwiftUI / WidgetKit
  packaging/             # Entitlements, signing, notices, update manifest
tests/                   # Protocol, adversarial devices, integration, graphics
tools/                   # Reproducible host/image builds and evidence capture
```

The layout remains the target architecture. The [foundation implementation record](../implementation/01-g0-foundation.md) lists the crates implemented so far and the unmet G0 gates.

## Work contracts

| Workstream | First task | Depends on | Failure handling |
| --- | --- | --- | --- |
| VMM | Boot/device inventory and unsafe-boundary tests | Exact image lock | Scope/fix unsupported HALs; compare QEMU/HVF if architecture fails |
| Graphics | GLES/Vulkan correctness and P0 measurements | Guest/host driver match | Explicit software recovery; block gaming claims on failing tests |
| Guest | Task/display bridge and authenticated handshake | Image customization | Reconcile actual task state; keep recovery control display visible on request |
| Host | State machine and same-user runtime service | Protocol schema | Preserve durable desired state; bounded retry and repair actions |
| UI | GPUI views backed by real runtime state | Model/error taxonomy | No optimistic success without backend confirmation |
| Integration | Launcher identity, IME, file broker and widgets | Native adapter and guest bridge | Clear partial-success state and targeted repair |
| Release | Reproducible packaging and license manifest | All dependency locks | Reject missing provenance/signatures; keep previous runtime usable |

## Acceptance matrix

| Scenario | Evidence required |
| --- | --- |
| Fresh install and missing runtime | Signed download/hash checks, interrupted-download resume, measured disk calculation |
| APK double-click while stopped | Installer receipt, start coalescing, package confirmation, working Mac entry |
| Unsupported ABI / high minSDK / bad signer | Blocked preflight and correct actionable message; no replaced app/data |
| Duplicate install / lost connection / cancel | Exactly one package commit; reconciled state after process restart |
| Two apps and multi-task app | Independent windows, Dock identity and task IDs; no cross-input/pixels |
| Dialogs and intents | Permissions, keyboard, chooser, notification and deep-link routing on correct display |
| Resize and screen change | Density/aspect correctness; no use-after-free or stale input coordinates |
| Root enable/disable | Separate Developer data, reboot confirmation, verified effective state and revoked sessions |
| VM crash / force stop | Durable app data; accurate broken-state UI; no leftover audio/input capture |
| Renderer loss | Generation invalidation; restart/software recovery; GPU mode label remains truthful |
| Widgets | Binding, configuration, resize, stale state, provider uninstall and action launch |
| Host integration | TCC denial, expired grants, no clipboard loop, notification identity and media focus |
| Upgrade and rollback | Matching host/guest protocol, signed atomic update and compatible data restore |
| Security boundaries | Fuzzed archive/device/protocol inputs; unauthorized shim rejected; no exposed ADB |
| Accessibility | Keyboard-only center; VoiceOver labels and documented guest accessibility limits |
| Distribution | Notarization, fresh-account Gatekeeper, widgets, launcher indexing and helper removal |

Use pure Rust tests for state/authorization, fuzzers for ZIP/protocol/Virtio parsers, guest instrumentation for task/IME/widgets, and physical Mac automation for UI/GPU/signing. Linux Cuttlefish CI is the guest reference, not a substitute for Mac runtime validation. Add tests for behavioral contracts and regressions, not for every static UI string.

## v0.1 release definition

A user can install a compatible ARM64 APK by double-clicking, open it from a Mac entry into its own window, run two apps concurrently, manage Android from the center, select a visible graphics mode and recover from a crash. At least a defined subset of games has measured hardware-accelerated support. The Developer profile exposes controlled guest root-shell management. Live widgets work for qualified providers; native WidgetKit is included only after its distribution gate. Source, licenses and reproducible build instructions accompany the free binary.

Any unmet item is explicitly removed from release claims or blocks the beta. The HTML prototype is a design acceptance artifact, not evidence that this release definition has been achieved.
