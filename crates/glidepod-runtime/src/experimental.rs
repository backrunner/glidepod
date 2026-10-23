//! Opt-in adapter for the externally built Avo HVF evaluation engine.
//! No engine or image is bundled. The management process owns the session lifetime.
use crate::{ImageFile, Store};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use glidepod_core::{Lifecycle, ProfileId, ProfileSettings, Renderer, VmState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const ENGINE_REVISION: &str = "99153258763a9ccdf5d4c6de36bd631e4dd0dded";
const LIMIT: u64 = 64 * 1024;
const BOOT_TIMEOUT: Duration = Duration::from_secs(120);
const STOP_TIMEOUT: Duration = Duration::from_secs(15);
const REQUIRED: &[&str] = &[
    "boot.img",
    "init_boot.img",
    "vendor_boot.img",
    "super.img",
    "userdata.img",
    "vbmeta.img",
    "vbmeta_system.img",
    "vbmeta_system_dlkm.img",
    "vbmeta_vendor_dlkm.img",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentalRuntime {
    pub schema_version: u32,
    pub engine: PathBuf,
    pub engine_sha256: String,
    pub engine_revision: String,
    pub images: PathBuf,
    pub files: Vec<ImageFile>,
    pub expected_fingerprint: String,
}
impl ExperimentalRuntime {
    pub fn from_local(engine: &Path, images: &Path, fingerprint: String) -> Result<Self> {
        let engine = engine.canonicalize()?;
        let images = images.canonicalize()?;
        let mut files = Vec::new();
        for name in REQUIRED {
            let path = images.join(name);
            ensure!(!path.is_symlink(), "Image sources must not be symlinks");
            files.push(ImageFile {
                path: name.to_string(),
                bytes: path.metadata()?.len(),
                sha256: digest(&path)?,
            });
        }
        let result = Self {
            schema_version: 1,
            engine_sha256: digest(&engine)?,
            engine,
            images,
            files,
            expected_fingerprint: fingerprint,
            engine_revision: ENGINE_REVISION.into(),
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.engine_revision == ENGINE_REVISION,
            "Unsupported experimental engine revision"
        );
        ensure!(
            self.engine.is_absolute() && self.images.is_absolute(),
            "Runtime paths must be absolute"
        );
        ensure!(
            !self.expected_fingerprint.is_empty()
                && self.expected_fingerprint.len() <= 512
                && !self.expected_fingerprint.contains(['\r', '\n']),
            "Invalid build fingerprint"
        );
        ensure!(
            self.files.len() == REQUIRED.len(),
            "Incomplete image inventory"
        );
        for name in REQUIRED {
            ensure!(
                self.files.iter().filter(|f| f.path == *name).count() == 1,
                "Missing or repeated image: {name}"
            );
        }
        Ok(())
    }
    fn verify(&self) -> Result<()> {
        self.validate()?;
        ensure!(
            digest(&self.engine)? == self.engine_sha256,
            "The engine changed. Register the reviewed build again."
        );
        for entry in &self.files {
            let path = self.images.join(&entry.path);
            ensure!(
                !path.is_symlink() && path.metadata()?.len() == entry.bytes,
                "Image size or path changed: {}",
                entry.path
            );
            ensure!(
                digest(&path)? == entry.sha256,
                "Image digest changed: {}",
                entry.path
            );
        }
        Ok(())
    }
}
pub(crate) fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("Open {}", path.display()))?;
    ensure!(file.metadata()?.is_file(), "Expected a regular file");
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub vm: VmState,
    pub configured: bool,
    pub profile: Option<ProfileId>,
    pub effective_root: bool,
    pub effective_renderer: Option<Renderer>,
    pub resources: Option<ProfileSettings>,
    pub fingerprint: Option<String>,
    pub engine_pid: Option<u32>,
    #[serde(default)]
    pub endpoints: Option<crate::guest::Endpoints>,
    pub message: String,
}
impl RuntimeStatus {
    pub fn label(&self) -> &'static str {
        match self.vm {
            VmState::MissingImage => "Not configured",
            VmState::Stopped => "Stopped",
            VmState::Starting { .. } => "Starting…",
            VmState::Running { .. } => "Running",
            VmState::Stopping { .. } => "Stopping…",
            VmState::Failed { .. } => "Failed",
        }
    }
    pub fn active(&self) -> bool {
        matches!(
            self.vm,
            VmState::Starting { .. } | VmState::Running { .. } | VmState::Stopping { .. }
        )
    }
    fn idle(configured: bool) -> Self {
        Self {
            vm: if configured {
                VmState::Stopped
            } else {
                VmState::MissingImage
            },
            configured,
            profile: None,
            effective_root: false,
            effective_renderer: None,
            resources: None,
            fingerprint: None,
            engine_pid: None,
            endpoints: None,
            message: if configured {
                "Experimental runtime ready. Developer profile only."
            } else {
                "No experimental runtime registered."
            }
            .into(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Request {
    Status,
    Stop,
}

impl Store {
    pub(crate) fn session_lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.root().join("session.lock"))?;
        file.try_lock_exclusive()
            .context("An Android session is already starting or running")?;
        Ok(file)
    }
    pub fn experimental_runtime(&self) -> Result<Option<ExperimentalRuntime>> {
        let path = self.root().join("experimental-runtime.json");
        if !path.exists() {
            return Ok(None);
        }
        let result: ExperimentalRuntime = read_json(&path)?;
        result.validate()?;
        Ok(Some(result))
    }
    pub fn register_experimental(&self, runtime: &ExperimentalRuntime) -> Result<()> {
        let _lock = self.session_lock()?;
        runtime.verify()?;
        atomic_json(&self.root().join("experimental-runtime.json"), runtime)?;
        atomic_json(
            &self.root().join("session-status.json"),
            &RuntimeStatus::idle(true),
        )
    }
    pub fn runtime_status(&self) -> Result<RuntimeStatus> {
        // Query the live owner first. A status file alone never proves liveness.
        if let Ok(status) = self.request(Request::Status) {
            return Ok(status);
        }
        let configured = self.experimental_runtime()?.is_some();
        let session_lock = self.session_lock();
        if let Err(error) = &session_lock {
            let busy = error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock);
            if !busy {
                return Err(anyhow::anyhow!(
                    "Cannot inspect runtime ownership: {error:#}"
                ));
            }
            return Ok(RuntimeStatus {
                vm: VmState::Starting { epoch: 0 },
                message: "Preparing the runtime…".into(),
                ..RuntimeStatus::idle(configured)
            });
        }
        let path = self.root().join("session-status.json");
        if path.exists() {
            let mut status: RuntimeStatus = read_json(&path)?;
            if status.active() {
                status.vm = VmState::Failed {
                    message:
                        "The runtime owner disconnected. Engine cleanup may still be in progress."
                            .into(),
                };
                status.message =
                    "The runtime owner disconnected. Engine cleanup may still be in progress."
                        .into();
                status.effective_root = false;
                status.engine_pid = None;
            }
            status.configured = configured;
            return Ok(status);
        }
        Ok(RuntimeStatus::idle(configured))
    }
    fn request(&self, request: Request) -> Result<RuntimeStatus> {
        let mut socket = UnixStream::connect(self.root().join("session.sock"))?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        socket.set_write_timeout(Some(Duration::from_millis(500)))?;
        let mut message = serde_json::to_vec(&request)?;
        message.push(b'\n');
        socket.write_all(&message)?;
        let mut line = String::new();
        BufReader::new(socket)
            .take(LIMIT + 1)
            .read_line(&mut line)?;
        ensure!(
            line.len() as u64 <= LIMIT && line.ends_with('\n'),
            "Invalid supervisor response"
        );
        Ok(serde_json::from_str(&line)?)
    }
    pub fn stop_vm(&self) -> Result<RuntimeStatus> {
        self.request(Request::Stop)
            .context("No responsive Android session to stop")
    }
    pub fn start_experimental(&self, helper: &Path) -> Result<RuntimeOwner> {
        let c = self.load()?;
        validate_start(
            c.active_profile,
            c.settings(),
            self.experimental_gpu()?.is_some(),
        )?;
        ensure!(
            self.experimental_runtime()?.is_some(),
            "Register an experimental runtime first"
        );
        let _lock = self.session_lock()?;
        // The worker acquires this same lock after spawn, before touching disks.
        drop(_lock);
        let log = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(self.root().join("supervisor.log"))?;
        let mut child = Command::new(helper)
            .arg("--data-dir")
            .arg(self.root())
            .arg("runtime-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .context("Start runtime supervisor")?;
        let input = child.stdin.take().context("Supervisor input missing")?;
        // Reap the helper even after its UI owner has closed. EOF requests shutdown.
        thread::spawn(move || {
            let _ = child.wait();
        });
        let owner = RuntimeOwner { input: Some(input) };
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.session_lock().is_ok() {
            ensure!(
                Instant::now() < deadline,
                "Supervisor did not acquire the session lock. See supervisor.log."
            );
            thread::sleep(Duration::from_millis(25));
        }
        Ok(owner)
    }
}
/// Keeping this pipe open keeps the session alive. Closing/crashing the UI yields EOF.
pub struct RuntimeOwner {
    input: Option<ChildStdin>,
}
impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        self.input.take();
    }
}
fn validate_start(
    profile: ProfileId,
    settings: &ProfileSettings,
    gpu_registered: bool,
) -> Result<()> {
    settings.validate(profile)?;
    ensure!(
        profile == ProfileId::Developer,
        "This userdebug image requires the Developer profile; its console has guest root access"
    );
    ensure!(
        settings.renderer != Renderer::Metal || gpu_registered,
        "Register an external experimental GPU renderer before selecting Metal."
    );
    Ok(())
}
pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("Missing parent")?)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= LIMIT,
        "Runtime record exceeds size limit"
    );
    Ok(serde_json::from_reader(file)?)
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
struct SocketPath(PathBuf);
impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Runs inside the signed helper. stdin EOF is a lifetime signal, never a guest shell command.
pub fn run_worker(store: &Store) -> Result<()> {
    let _lock = store.session_lock()?;
    let result = worker_inner(store);
    if let Err(error) = &result {
        let mut status = RuntimeStatus::idle(true);
        status.vm = VmState::Failed {
            message: format!("{error:#}"),
        };
        status.message = format!("{error:#}");
        atomic_json(&store.root().join("session-status.json"), &status)?;
    }
    result
}
fn worker_inner(store: &Store) -> Result<()> {
    let config = store.load()?;
    validate_start(
        config.active_profile,
        config.settings(),
        store.experimental_gpu()?.is_some(),
    )?;
    let runtime = store
        .experimental_runtime()?
        .context("No experimental runtime registered")?;
    let settings = config.settings().clone();
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let mut lifecycle = Lifecycle::new(true);
    let generation = lifecycle.request_start()?;
    let mut status = RuntimeStatus {
        vm: VmState::Starting { epoch },
        profile: Some(ProfileId::Developer),
        resources: Some(settings.clone()),
        message: "Verifying engine and image files…".into(),
        ..RuntimeStatus::idle(true)
    };
    atomic_json(&store.root().join("session-status.json"), &status)?;
    let (closed_tx, closed_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut sink = [0u8; 128];
        while matches!(std::io::stdin().read(&mut sink), Ok(1..)) {}
        let _ = closed_tx.send(());
    });
    runtime.verify()?;
    // Do not boot after an owner disappeared during image verification.
    if closed_rx.try_recv().is_ok() {
        atomic_json(
            &store.root().join("session-status.json"),
            &RuntimeStatus::idle(true),
        )?;
        return Ok(());
    }
    let socket_path = store.root().join("session.sock");
    ensure!(
        socket_path.as_os_str().len() < 104,
        "Data directory is too long for a local control socket"
    );
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }
    let listener = UnixListener::bind(&socket_path)?;
    let _socket = SocketPath(socket_path.clone());
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let sockets = tempfile::Builder::new()
        .prefix("gpvm-")
        .tempdir_in("/tmp")?;
    let instance = store.root().join("developer/instance");
    fs::create_dir_all(&instance)?;
    fs::set_permissions(
        store.root().join("developer"),
        fs::Permissions::from_mode(0o700),
    )?;
    fs::set_permissions(&instance, fs::Permissions::from_mode(0o700))?;
    let log = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(store.root().join("engine.log"))?;
    let gpu = if settings.renderer == Renderer::Metal {
        store.experimental_gpu()?
    } else {
        None
    };
    if let Some(gpu) = &gpu {
        gpu.verify()?;
    }
    let mut command = Command::new(&runtime.engine);
    command.arg("cuttlefish").arg(&runtime.images);
    if let Some(gpu) = &gpu {
        command
            .args(["--gpu", "gfxstream", "--gfxstream-dir"])
            .arg(&gpu.directory);
    } else {
        command.args(["--gpu", "2d"]);
    }
    let process = command
        .arg("--instance")
        .arg(&instance)
        .arg("--runtime")
        .arg(sockets.path())
        .args([
            "--cpus",
            &settings.vcpus.to_string(),
            "--mem",
            &settings.memory_mib.to_string(),
            "--display",
            "720x1280",
            "--exit-on-reboot",
            "--no-microphone",
            "--cameras",
            "none",
        ])
        .env(
            "AVO_VMM_MUTE",
            if config.audio_enabled && std::env::var("GLIDEPOD_MUTE_AUDIO").as_deref() != Ok("1") {
                "0"
            } else {
                "1"
            },
        )
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    let mut process = OwnedChild(process);
    status.engine_pid = Some(process.0.id());
    status.message = "Waiting for Android to finish starting…".into();
    let started = Instant::now();
    let mut console: Option<UnixStream> = None;
    let mut console_lines = ConsoleLines::default();
    let nonce = format!("GP{epoch}_{}", process.0.id());
    let mut last_query = Instant::now() - Duration::from_secs(4);
    let mut stop_at: Option<Instant> = None;
    let mut stop_sent = false;
    let mut failure = None;
    loop {
        // Keep shutdown independent of guest readiness and the UI thread.
        if closed_rx.try_recv().is_ok() {
            begin_stop(&mut lifecycle, &mut status, epoch, &mut stop_at)?;
        }
        for _ in 0..8 {
            let Ok((mut client, _)) = listener.accept() else {
                break;
            };
            // macOS can reject socket options after a peer exits between connect and accept.
            // A broken management connection has no authority over VM lifetime.
            if client.set_nonblocking(false).is_err()
                || client
                    .set_read_timeout(Some(Duration::from_millis(150)))
                    .is_err()
                || client
                    .set_write_timeout(Some(Duration::from_millis(150)))
                    .is_err()
            {
                continue;
            }
            let mut line = String::new();
            let parsed = BufReader::new(&mut client).take(1025).read_line(&mut line);
            if parsed.is_ok() && line.len() <= 1024 && line.ends_with('\n') {
                match serde_json::from_str::<Request>(&line) {
                    Ok(Request::Stop) => {
                        begin_stop(&mut lifecycle, &mut status, epoch, &mut stop_at)?
                    }
                    Ok(Request::Status) => {}
                    Err(_) => continue,
                }
                // A client closing its connection must not kill the VM.
                if let Ok(mut message) = serde_json::to_vec(&status) {
                    message.push(b'\n');
                    let _ = client.write_all(&message);
                }
            }
        }
        if let Some(exit) = process
            .0
            .try_wait()
            .context("Read owned engine exit status")?
        {
            let expected = stop_at.is_some() && exit.success() && failure.is_none();
            lifecycle.process_exited(generation, expected)?;
            status.vm = if expected {
                VmState::Stopped
            } else {
                VmState::Failed {
                    message: failure
                        .clone()
                        .unwrap_or_else(|| format!("Android exited ({exit}). See engine.log.")),
                }
            };
            status.message = match &status.vm {
                VmState::Failed { message } => message.clone(),
                _ => "Android stopped.".into(),
            };
            status.effective_root = false;
            status.engine_pid = None;
            atomic_json(&store.root().join("session-status.json"), &status)?;
            return Ok(());
        }
        if console.is_none()
            && let Ok(stream) = UnixStream::connect(sockets.path().join("console.sock"))
            && stream.set_nonblocking(true).is_ok()
        {
            console = Some(stream);
        }
        if let Some(stream) = console.as_mut() {
            if stop_at.is_some() && !stop_sent {
                stop_sent = stream.write_all(b"\nreboot -p\n").is_ok();
            } else if stop_at.is_none()
                && !matches!(status.vm, VmState::Running { .. })
                && last_query.elapsed() >= Duration::from_secs(2)
            {
                let query = format!(
                    "\necho {nonce}:$(getprop sys.boot_completed):$(su 0 id -u):$(getprop ro.build.fingerprint)\n"
                );
                if stream.write_all(query.as_bytes()).is_ok() {
                    last_query = Instant::now();
                }
            }
            let mut bytes = [0u8; 8192];
            match stream.read(&mut bytes) {
                Ok(0) => console = None,
                Ok(n) => {
                    for line in console_lines.feed(&bytes[..n])? {
                        if let Some(fingerprint) = ready_fingerprint(&line, &nonce) {
                            if fingerprint != runtime.expected_fingerprint {
                                failure = Some(
                                    "Guest build fingerprint does not match the registered image."
                                        .into(),
                                );
                                begin_stop(&mut lifecycle, &mut status, epoch, &mut stop_at)?;
                            } else if matches!(lifecycle.state, VmState::Starting { .. }) {
                                lifecycle.guest_ready(generation, ProfileId::Developer, true)?;
                                status.vm = VmState::Running { epoch };
                                status.endpoints = Some(crate::guest::Endpoints {
                                    epoch,
                                    vsock: sockets.path().join("vsock.sock"),
                                    control: sockets.path().join("control.sock"),
                                    frames: instance.join("frames"),
                                });
                                status.effective_root = true;
                                status.effective_renderer = Some(if gpu.is_some() {
                                    Renderer::Metal
                                } else {
                                    Renderer::Software
                                });
                                status.fingerprint = Some(fingerprint.into());
                                status.message = if gpu.is_some() { "Android is running with experimental Metal rendering. Game compatibility is not qualified." } else { "Android is running with software rendering. Guest root access verified." }.into();
                                atomic_json(&store.root().join("session-status.json"), &status)?;
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => console = None,
            }
        }
        if stop_at.is_none()
            && !matches!(status.vm, VmState::Running { .. })
            && started.elapsed() > BOOT_TIMEOUT
        {
            failure = Some("Android did not finish starting within 120 seconds.".into());
            begin_stop(&mut lifecycle, &mut status, epoch, &mut stop_at)?;
        }
        if let Some(at) = stop_at
            && at.elapsed() > STOP_TIMEOUT
        {
            process.0.kill().context("Stop timed-out engine")?;
            process.0.wait().context("Reap stopped engine")?;
            status.vm = VmState::Failed {
                message: failure.unwrap_or_else(|| {
                    "Android did not shut down within 15 seconds and was force-stopped.".into()
                }),
            };
            status.message = match &status.vm {
                VmState::Failed { message } => message.clone(),
                _ => unreachable!(),
            };
            status.effective_root = false;
            status.engine_pid = None;
            atomic_json(&store.root().join("session-status.json"), &status)?;
            return Ok(());
        }
        // Wake immediately for management requests. A fixed sleep here adds its full
        // duration to every epoch check performed by each frame and input stream.
        use std::os::fd::AsRawFd;
        let mut ready = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialized pollfd referring to our live listener; bounded wait.
        unsafe {
            libc::poll(&mut ready, 1, 40);
        }
    }
}
fn begin_stop(
    lifecycle: &mut Lifecycle,
    status: &mut RuntimeStatus,
    epoch: u64,
    stop_at: &mut Option<Instant>,
) -> Result<()> {
    if stop_at.is_none() {
        lifecycle.request_stop()?;
        *stop_at = Some(Instant::now());
        status.vm = VmState::Stopping { epoch };
        status.message = "Shutting down Android…".into();
    }
    Ok(())
}
#[derive(Default)]
struct ConsoleLines {
    pending: Vec<u8>,
}
impl ConsoleLines {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.pending.extend_from_slice(bytes);
        ensure!(self.pending.len() <= 65536, "Console line exceeds limit");
        let Some(end) = self.pending.iter().rposition(|b| *b == b'\n') else {
            return Ok(Vec::new());
        };
        let data: Vec<_> = self.pending.drain(..=end).collect();
        Ok(String::from_utf8_lossy(&data)
            .lines()
            .map(|s| s.trim_end_matches('\r').to_owned())
            .collect())
    }
}
fn ready_fingerprint<'a>(line: &'a str, nonce: &str) -> Option<&'a str> {
    // Exact complete line rejects the shell's echoed command and stale output.
    line.strip_prefix(&format!("{nonce}:1:0:"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_requires_complete_fresh_reply_and_root() {
        let mut reader = ConsoleLines::default();
        assert!(
            reader
                .feed(b"echo GP1:$(getprop sys.boot_completed):$(id -u):fp\r\nGP1:1:")
                .unwrap()
                .iter()
                .all(|l| ready_fingerprint(l, "GP1").is_none())
        );
        let lines = reader
            .feed(b"0:build\r\nGP0:1:0:old\nGP1:1:2000:no-root\n")
            .unwrap();
        assert_eq!(ready_fingerprint(&lines[0], "GP1"), Some("build"));
        assert!(ready_fingerprint(&lines[1], "GP1").is_none());
        assert!(ready_fingerprint(&lines[2], "GP1").is_none());
    }
    #[test]
    fn console_memory_is_bounded() {
        assert!(ConsoleLines::default().feed(&vec![b'x'; 65537]).is_err());
    }
    #[test]
    fn personal_and_metal_are_rejected_before_spawning() {
        assert!(validate_start(ProfileId::Personal, &ProfileSettings::default(), false).is_err());
        assert!(
            validate_start(
                ProfileId::Developer,
                &ProfileSettings {
                    renderer: Renderer::Metal,
                    ..ProfileSettings::default()
                },
                false
            )
            .is_err()
        );
    }
    #[test]
    fn stale_running_file_never_claims_live_vm() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().into()).unwrap();
        let mut state = RuntimeStatus::idle(false);
        state.vm = VmState::Running { epoch: 99 };
        state.effective_root = true;
        atomic_json(&store.root().join("session-status.json"), &state).unwrap();
        let observed = store.runtime_status().unwrap();
        assert!(matches!(observed.vm, VmState::Failed { .. }));
        assert!(!observed.effective_root);
    }
    #[test]
    fn instance_lock_is_exclusive_and_released() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().into()).unwrap();
        let held = store.session_lock().unwrap();
        assert!(store.session_lock().is_err());
        drop(held);
        assert!(store.session_lock().is_ok());
    }
    #[test]
    fn owned_process_is_reaped_on_error_or_drop() {
        let child = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        drop(OwnedChild(child));
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
}
