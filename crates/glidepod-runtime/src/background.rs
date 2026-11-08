//! One small per-store agent owns VM lifetime and notifications independently of windows.
use crate::{
    Store,
    experimental::{RuntimeOwner, RuntimeStatus},
    notification_relay::NotificationBridge,
    notifications::AndroidNotification,
};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use glidepod_core::{MemoryMode, RuntimeMode, VmState};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const IDLE_DELAY: Duration = Duration::from_secs(180);
static SHOW_TRAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static TRAY_ACTIONS: Mutex<Vec<glidepod_macos::tray::Action>> = Mutex::new(Vec::new());
thread_local! {
    static TRAY: std::cell::RefCell<Option<glidepod_macos::tray::Tray>> = const { std::cell::RefCell::new(None) };
}
extern "C" fn tick_tray() {
    TRAY.with(|tray| {
        let mut tray = tray.borrow_mut();
        if SHOW_TRAY.load(std::sync::atomic::Ordering::Relaxed) {
            if tray.is_none() {
                *tray = glidepod_macos::tray::Tray::new().ok();
            }
            if let Some(tray) = tray.as_ref() {
                let mut actions = TRAY_ACTIONS.lock().unwrap();
                let room = 16usize.saturating_sub(actions.len());
                actions.extend(tray.drain().into_iter().take(room));
            }
        } else {
            tray.take();
        }
    });
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Request {
    Status,
    Wake,
    Stop,
    Authorize,
    Events,
    Quit,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub pid: u32,
    pub managed: bool,
    pub reason: String,
    pub notifications: String,
    pub memory: Option<crate::memory::MemoryStatus>,
    pub runtime: RuntimeStatus,
    #[serde(default)]
    pub opens: Vec<(u64, AndroidNotification)>,
    pub error: Option<String>,
}
fn lock(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    f.try_lock_exclusive()?;
    Ok(f)
}
impl Store {
    fn background_request(&self, request: Request) -> Result<Status> {
        let mut socket = UnixStream::connect(self.root().join("background.sock"))?;
        socket.set_read_timeout(Some(Duration::from_secs(8)))?;
        socket.set_write_timeout(Some(Duration::from_secs(1)))?;
        writeln!(socket, "{}", serde_json::to_string(&request)?)?;
        let mut line = String::new();
        BufReader::new(socket)
            .take(256 * 1024 + 1)
            .read_line(&mut line)?;
        ensure!(
            line.len() <= 256 * 1024 && line.ends_with('\n'),
            "Invalid background reply"
        );
        let status: Status = serde_json::from_str(&line)?;
        ensure!(
            status.error.is_none(),
            "{}",
            status.error.as_deref().unwrap_or_default()
        );
        Ok(status)
    }
    pub fn background_status(&self) -> Result<Status> {
        self.background_request(Request::Status)
    }
    pub fn background_events(&self) -> Result<Status> {
        self.background_request(Request::Events)
    }
    pub fn quit_background(&self) -> Result<()> {
        fs::write(
            self.root().join("background-paused"),
            b"Stopped explicitly.\n",
        )?;
        if let Err(error) = self.background_request(Request::Quit) {
            if self.runtime_status()?.active() {
                self.stop_vm()?;
            }
            if !error.downcast_ref::<std::io::Error>().is_some_and(|e| {
                matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                )
            }) {
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn stop_background_vm(&self) -> Result<RuntimeStatus> {
        // Persist intent even if the agent is busy or its socket is unavailable.
        fs::write(
            self.root().join("background-paused"),
            b"Stopped explicitly.\n",
        )?;
        if self.background_status().is_ok() {
            Ok(self.background_request(Request::Stop)?.runtime)
        } else {
            self.stop_vm()
        }
    }
    pub fn start_background_vm(&self) -> Result<()> {
        self.ensure_background()?;
        self.background_request(Request::Wake)?;
        Ok(())
    }
    pub fn request_notification_authorization(&self) -> Result<()> {
        self.ensure_background()?;
        self.background_request(Request::Authorize)?;
        Ok(())
    }
    pub fn ensure_background(&self) -> Result<()> {
        if self.background_status().is_ok() {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let _start = loop {
            match lock(&self.root().join("background-start.lock")) {
                Ok(lock) => break lock,
                Err(e)
                    if Instant::now() < deadline
                        && e.downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock) =>
                {
                    thread::sleep(Duration::from_millis(25))
                }
                Err(e) => return Err(e),
            }
        };
        if self.background_status().is_ok() {
            return Ok(());
        }
        let exe = std::env::current_exe()?;
        let embedded = exe.parent().and_then(Path::parent).map(|p| {
            p.join("Library/LoginItems/GlidePod Background.app/Contents/MacOS/glidepodctl")
        });
        let helper = embedded
            .filter(|p| p.is_file())
            .unwrap_or_else(|| exe.with_file_name("glidepodctl"));
        let log = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(self.root().join("background.log"))?;
        let mut child = Command::new(helper)
            .arg("--data-dir")
            .arg(self.root())
            .arg("background-worker")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        loop {
            if self.background_status().is_ok() {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "Background service did not start. See background.log."
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
}

pub(crate) fn window_active(store: &Store) -> Result<bool> {
    let dir = store.root().join("developer/windows");
    if !dir.exists() {
        return Ok(false);
    }
    for n in 1..16 {
        let path = dir.join(format!("display-{n}.lock"));
        if !path.exists() {
            continue;
        }
        match lock(&path) {
            Ok(_) => {}
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock) =>
            {
                return Ok(true);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}
/// No CPU-usage heuristics: a waiting push connection is still background work.
pub fn keep_reason(
    mode: RuntimeMode,
    notifications: bool,
    explicit: bool,
    windows: bool,
    busy: Option<bool>,
) -> Option<&'static str> {
    if mode == RuntimeMode::Resident {
        Some("Always on: Android and background services stay online.")
    } else if notifications || explicit {
        Some("Keeping Android online for background apps and notifications.")
    } else if windows {
        Some("An app window is open.")
    } else if busy == Some(true) {
        Some("Android has background work.")
    } else if busy.is_none() {
        Some("Background activity is unknown; keeping Android online.")
    } else {
        None
    }
}
fn guest_busy(store: &Store) -> Result<bool> {
    let g = store.guest()?;
    // Typed system queries include foreground services, playback, downloads and enabled push infrastructure.
    let result = g.bridge("dev.glidepod.bridge.Background", &[])?;
    #[derive(Deserialize)]
    struct Activity {
        busy: bool,
    }
    Ok(serde_json::from_str::<Activity>(result.trim())?.busy)
}

pub fn run(store: Store) -> Result<()> {
    let _lock = lock(&store.root().join("background.lock"))
        .context("A background agent already owns this store")?;
    thread::spawn(move || {
        let _lock = _lock;
        if let Err(e) = worker(&store) {
            eprintln!("Background agent: {e:#}");
        }
        glidepod_macos::notifications::stop_background_loop();
    });
    glidepod_macos::notifications::run_background_loop(tick_tray);
    Ok(())
}
fn worker(store: &Store) -> Result<()> {
    let path = store.root().join("background.sock");
    ensure!(
        path.as_os_str().len() < 104,
        "Background socket path is too long"
    );
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let relay = NotificationBridge::new(store.clone(), Arc::new(Mutex::new(Vec::new())));
    let helper = std::env::current_exe()?;
    let mut owner: Option<RuntimeOwner> = None;
    let paused_path = store.root().join("background-paused");
    let mut manually_stopped = paused_path.exists();
    let mut idle_since = Instant::now();
    let mut sampled = Instant::now() - Duration::from_secs(30);
    let mut shrink_at = Instant::now();
    let mut memory = None;
    let mut memory_epoch = 0;
    let mut idle_compaction = crate::memory::IdleCompaction::default();
    let mut reason = "Ready to open Android apps.".to_string();
    let mut runtime = store.runtime_status()?;
    let mut open_requested = false;
    let mut desktop_seen: Option<Instant> = None;
    let mut quitting = false;
    let mut wake_pending = false;
    let mut sampler = glidepod_macos::process_usage::Sampler::default();
    loop {
        for action in std::mem::take(&mut *TRAY_ACTIONS.lock().unwrap()) {
            match action {
                glidepod_macos::tray::Action::OpenCenter
                | glidepod_macos::tray::Action::FocusApp { .. } => {
                    if let Some(app) = host_bundle(&helper) {
                        let _ =
                            glidepod_macos::notifications::open_background_host(&app, store.root());
                    }
                }
                glidepod_macos::tray::Action::Quit => {
                    fs::write(&paused_path, b"Stopped explicitly.\n")?;
                    if owner.is_some() {
                        let _ = store.stop_vm();
                    }
                    owner.take();
                    quitting = true;
                }
            }
        }
        for _ in 0..16 {
            let Ok((mut client, _)) = listener.accept() else {
                break;
            };
            // Darwin inherits O_NONBLOCK on accept; a client may still be writing its request.
            if client.set_nonblocking(false).is_err()
                || client
                    .set_read_timeout(Some(Duration::from_millis(200)))
                    .is_err()
                || client
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .is_err()
            {
                continue;
            }
            let mut line = String::new();
            if BufReader::new(&mut client)
                .take(1025)
                .read_line(&mut line)
                .is_err()
                || line.len() > 1024
                || !line.ends_with('\n')
            {
                continue;
            }
            let Ok(request) = serde_json::from_str::<Request>(&line) else {
                continue;
            };
            if matches!(request, Request::Events) {
                desktop_seen = Some(Instant::now());
            }
            let action: Result<()> = (|| {
                match request {
                    Request::Wake => {
                        if paused_path.exists() {
                            fs::remove_file(&paused_path)?;
                        }
                        manually_stopped = false;
                        idle_since = Instant::now();
                        runtime = store.runtime_status()?;
                        if matches!(runtime.vm, VmState::Stopping { .. }) {
                            wake_pending = true;
                            reason =
                                "Finishing shutdown; Android will reopen automatically.".into();
                            return Ok(());
                        }
                        wake_pending = false;
                        if !runtime.active() {
                            owner = Some(store.start_experimental(&helper)?);
                        }
                    }
                    Request::Stop | Request::Quit => {
                        fs::write(
                            &paused_path,
                            b"Stopped explicitly. An app launch resumes Android.\n",
                        )?;
                        manually_stopped = true;
                        wake_pending = false;
                        runtime = store.runtime_status()?;
                        if runtime.active() {
                            runtime = store.stop_vm()?;
                        }
                        owner.take();
                        quitting = matches!(request, Request::Quit);
                        reason = "Stopped by you. Open an app to start Android again.".into();
                    }
                    Request::Authorize => {
                        glidepod_macos::notifications::command(
                            serde_json::json!({"op":"request"}),
                        )?;
                    }
                    _ => {}
                }
                Ok(())
            })();
            let opens = if matches!(request, Request::Events) {
                relay.opens.lock().unwrap().clone()
            } else {
                Vec::new()
            };
            let open_count = opens.len();
            let status = Status {
                pid: std::process::id(),
                managed: owner.is_some(),
                reason: reason.clone(),
                notifications: relay.status.lock().unwrap().clone(),
                memory: memory.clone(),
                runtime: store.runtime_status().unwrap_or_else(|_| runtime.clone()),
                opens,
                error: action.err().map(|e| format!("{e:#}")),
            };
            if writeln!(client, "{}", serde_json::to_string(&status)?).is_ok() && open_count > 0 {
                relay.opens.lock().unwrap().drain(..open_count);
            }
        }
        if quitting {
            break;
        }
        SHOW_TRAY.store(
            desktop_seen.is_none_or(|t| t.elapsed() > Duration::from_secs(5)),
            std::sync::atomic::Ordering::Relaxed,
        );
        if sampled.elapsed() >= Duration::from_secs(5) {
            sampled = Instant::now();
            let c = store.load()?;
            manually_stopped |= paused_path.exists();
            runtime = store.runtime_status()?;
            let usage = runtime.engine_pid.and_then(|pid| {
                let engine = store.experimental_runtime().ok().flatten()?;
                sampler.sample(pid, &engine.engine).ok()
            });
            if usage.is_none() {
                sampler.reset();
            }
            glidepod_macos::tray::publish(&glidepod_macos::tray::Snapshot {
                state: runtime.label().into(),
                detail: reason.clone(),
                cpu: usage
                    .as_ref()
                    .and_then(|u| u.cpu_percent)
                    .map(|p| format!("{p:.1}%"))
                    .unwrap_or_else(|| "—".into()),
                memory: usage
                    .as_ref()
                    .map(|u| format!("{:.0} MiB", u.footprint_bytes as f64 / 1048576.))
                    .unwrap_or_else(|| "—".into()),
                allocation: runtime
                    .resources
                    .as_ref()
                    .filter(|_| runtime.active())
                    .map(|s| format!("{} vCPUs · {} MiB", s.vcpus, s.memory_mib))
                    .unwrap_or_else(|| "—".into()),
                app_count: Some(0),
                apps: Vec::new(),
            });
            if !runtime.active() {
                owner.take();
                memory = None;
                memory_epoch = 0;
                if !manually_stopped
                    && !matches!(runtime.vm, VmState::Failed { .. })
                    && (wake_pending
                        || c.runtime_mode == RuntimeMode::Resident
                        || c.notifications_enabled
                        || !c.background_apps.is_empty())
                    && runtime.configured
                    && c.active_profile == glidepod_core::ProfileId::Developer
                    && store.maintenance_lock(false).is_ok()
                {
                    match store.start_experimental(&helper) {
                        Ok(next) => {
                            wake_pending = false;
                            owner = Some(next);
                            idle_since = Instant::now();
                        }
                        Err(e) => {
                            wake_pending = false;
                            reason = format!("Background start failed: {e:#}");
                            manually_stopped = true;
                        }
                    }
                }
            } else if let VmState::Running { epoch } = runtime.vm {
                let windows = window_active(store).unwrap_or(true);
                let busy = if owner.is_some()
                    && !windows
                    && c.runtime_mode == RuntimeMode::Automatic
                    && !c.notifications_enabled
                    && c.background_apps.is_empty()
                {
                    guest_busy(store).ok()
                } else {
                    Some(false)
                };
                if let Some(keep) = keep_reason(
                    c.runtime_mode,
                    c.notifications_enabled,
                    !c.background_apps.is_empty(),
                    windows,
                    busy,
                ) {
                    idle_since = Instant::now();
                    reason = keep.into();
                } else {
                    reason = "No background work; waiting before stopping Android.".into();
                    if owner.is_some()
                        && idle_since.elapsed() >= IDLE_DELAY
                        && let Ok(_maintenance) = store.maintenance_lock(true)
                        && let Ok(_activity) = lock(&store.root().join("activity.lock"))
                    {
                        let _ = store.stop_vm();
                        owner.take();
                        reason =
                            "Android stopped to release memory. App icons remain available.".into();
                    }
                }
                if store.runtime_status().is_ok_and(
                    |s| matches!(s.vm, VmState::Running { epoch: current } if current == epoch),
                ) {
                    let update = (|| -> Result<_> {
                        let g = store.guest()?;
                        if memory_epoch != epoch {
                            let prepared = g.prepare_memory()?;
                            ensure!(
                                prepared.active && prepared.reporting,
                                "Android balloon driver is not ready"
                            );
                            memory_epoch = epoch;
                        }
                        let mut state = g.memory_status()?;
                        let sample = crate::memory::sample(store)?;
                        if owner.is_some() {
                            idle_compaction.tick(store, epoch, windows, sample);
                        }
                        if c.settings().memory_mode == MemoryMode::Manual {
                            if state.target_mib != 0 {
                                g.balloon_target(0)?;
                                state.target_mib = 0;
                            }
                            return Ok(state);
                        }
                        let slow = shrink_at.elapsed() >= Duration::from_secs(30)
                            && (!windows || glidepod_macos::memory_pressure() > 1);
                        let capacity = runtime
                            .resources
                            .as_ref()
                            .map(|s| s.memory_mib)
                            .unwrap_or(4096);
                        let target = crate::memory::next_target(
                            state.actual_mib.min(state.target_mib),
                            capacity,
                            sample,
                            slow,
                        );
                        if target != state.target_mib {
                            g.balloon_target(target)?;
                            state.target_mib = target;
                        }
                        if slow {
                            shrink_at = Instant::now();
                        }
                        Ok(state)
                    })();
                    match update {
                        Ok(state) => memory = Some(state),
                        Err(e) => {
                            memory = None;
                            reason.push_str(&format!(" Memory: {e:#}"));
                        }
                    }
                }
            }
            let pending = !relay.opens.lock().unwrap().is_empty();
            if pending
                && !open_requested
                && let Some(app) = host_bundle(&helper)
            {
                let _ = glidepod_macos::notifications::open_background_host(&app, store.root());
            }
            open_requested = pending;
        }
        use std::os::fd::AsRawFd;
        let mut ready = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe {
            libc::poll(&mut ready, 1, 250);
        }
    }
    fs::remove_file(path)?;
    Ok(())
}
fn host_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .last()
        .map(Path::to_path_buf)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_stop_push_services_on_cpu_inactivity() {
        assert!(keep_reason(RuntimeMode::Resident, false, false, false, Some(false)).is_some());
        assert!(keep_reason(RuntimeMode::Automatic, true, false, false, Some(false)).is_some());
        assert!(keep_reason(RuntimeMode::Automatic, false, true, false, Some(false)).is_some());
        assert!(keep_reason(RuntimeMode::Automatic, false, false, false, None).is_some());
        assert!(keep_reason(RuntimeMode::Automatic, false, false, false, Some(true)).is_some());
        assert!(keep_reason(RuntimeMode::Automatic, false, false, false, Some(false)).is_none());
    }
}
