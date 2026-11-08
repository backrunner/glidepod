# macOS menu bar monitoring

The desktop owns a native `NSStatusItem` and `NSMenu`. The menu shows Android's current state, VM process CPU and memory, assigned vCPUs/RAM, and the count and list of Android apps with an open GlidePod window. Selecting an app raises its existing window. **Open GlidePod…** returns to the management center; **Quit GlidePod** uses the same application exit path as the main menu. **GlidePod → VM Status** also opens the status menu. **File → Close Window** and Command-W close the current window.

Closing the management center or the last Android window no longer exits the desktop. Monitoring continues without any open windows. Explicit Quit terminates the desktop and its tray, following the runtime's existing shutdown policy. This tray does not introduce another VM owner or an additional daemon.

## Measurements and failure behavior

- A background thread reads the existing `Store::runtime_status` source and samples roughly once per second. It never queries Android over ADB or performs socket/filesystem work on AppKit's main thread. Opening the menu cannot start a VM.
- The registered VM executable path is checked against the engine PID before and after `proc_pid_rusage`. PID changes, process start-time changes, counter resets and read failures discard the CPU baseline. An initial valid reading shows **Measuring…** for CPU; unavailable values use a dash.
- CPU uses the difference between cumulative process user/system time divided by measured elapsed wall time. 100% means one fully occupied host core, so multiple busy cores can exceed 100%. XNU's rusage CPU counters use Mach absolute ticks; the native adapter converts them using `mach_timebase_info` with a 128-bit intermediate. The [Apple XNU task accounting implementation](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c) supplies those counters and the physical footprint.
- **VM memory** is the process's host memory footprint, including the VMM's overhead; it is not guest `MemAvailable` or an Android app's private heap. **Assigned** separately shows the vCPU/RAM configuration reported by the active runtime.
- **Open apps** counts valid native app windows for the current running VM epoch, including minimized windows. It excludes Android services/background-only processes and stale windows from an earlier VM session. Apps are sorted by label and selecting one focuses the corresponding package's window.
- Immutable snapshots cross into Objective-C under a lock. A native timer in common run-loop modes updates values while the menu is tracking. Action rows stay in place during tracking; obsolete app actions become disabled. No snapshot for five seconds changes the menu to **Status unavailable** and removes old numeric readings.
- Native actions use a bounded queue drained by the desktop's existing timer. The native object is main-thread-only, removes its status item and invalidates its timer on teardown; the worker receives a stop signal when its service is dropped.

## Validation, 2026-11-08

The focused Rust tests cover CPU interval arithmetic, PID/start identity resets, executable mismatch, real host memory sampling, current-epoch window counting, stopped/failed states and resource formatting. A short CPU workload compares converted native CPU totals with independent POSIX `getrusage` totals; this caught and prevents a 125/3 timebase undercount on the Apple Silicon validation host.

The workspace test run passed 91 tests, with 13 environment-dependent tests ignored by default; the live tray test below was then run explicitly. Workspace Clippy passed with warnings denied.

`python3 tools/check-tray-native.py` exercises the real AppKit adapter in-process: native icon/menu creation, background publication, Unicode app actions, focus/open/quit payloads and single consumption, event-tracking-mode refresh, stable action positions, stale/stopped states and timer/status-item teardown. These checks invoke the adapter's actual menu callbacks; they do not synthesize desktop clicks.

The opt-in `tray::tests::live_vm_process_metrics` check sampled the existing disposable `host-files-20261107` guest. It reported the running engine PID, a finite CPU percentage, a positive memory footprint and separate assigned resources. At that sample the VM was configured with 4 vCPUs/4 GiB and the process footprint was approximately 4.77 GiB. This is a point-in-time validation, not a performance benchmark. The output is stored locally at `evidence/tray-20261108/live-metrics.log`.

The development app was rebuilt and its strict ad-hoc bundle signature verified. The native **File → Close Window** action was invoked on the management center and the same desktop PID remained running. Opening Clock from Finder then produced a real Android window through that resident process. Closing Clock returned to the management center, which displayed zero open apps. The computer-use surface exposes application windows and the main application menu, but not the global status bar; a physical status-icon click and a captured status-menu screenshot remain outside the current UI evidence.

No commit, push, notarization or public release was performed.
