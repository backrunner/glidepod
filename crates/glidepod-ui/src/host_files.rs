//! Per-window file requests and a single revision-checked clipboard coordinator.
use anyhow::{Result, ensure};
use glidepod_macos::files::command;
use glidepod_runtime::{display::AppWindow, files::PickerRequest, guest::Guest};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct ClipboardState {
    epoch: u64,
    host: Option<i64>,
    guest: String,
    unsupported_host: bool,
}
static CLIPBOARD: Mutex<ClipboardState> = Mutex::new(ClipboardState {
    epoch: 0,
    host: None,
    guest: String::new(),
    unsupported_host: false,
});

fn current(lease: u64) -> bool {
    lease != 0 && crate::window_input::active_lease() == lease
}
struct ClipboardTemp(Option<PathBuf>);
impl Drop for ClipboardTemp {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Serialize transfers across windows, and compare both endpoints before publishing.
pub fn synchronize_clipboard(client: &Guest, lease: u64, paste: bool) -> Result<()> {
    let mut state = CLIPBOARD.lock().unwrap();
    if !current(lease) {
        return Ok(());
    }
    if state.epoch != client.endpoints.epoch {
        *state = ClipboardState {
            epoch: client.endpoints.epoch,
            ..Default::default()
        };
    }
    let host = command(json!({"op":"clipboard_status"}))?["revision"].as_i64();
    ensure!(
        !paste || state.host != host || !state.unsupported_host,
        "This clipboard format cannot be pasted into Android"
    );
    let guest = client.clipboard()?;
    let version = guest["version"].as_str().unwrap_or("");
    if guest["kind"] == "unavailable" && paste {
        state.host = None;
    }
    if state.host != host {
        let mut content = command(json!({"op":"clipboard_read"}))?;
        let _temporary = ClipboardTemp(content["temporary"].as_str().map(PathBuf::from));
        // Unsupported representations leave the other clipboard intact.
        if content["kind"] == "unsupported" {
            state.unsupported_host = true;
            state.host = host;
            state.guest = version.into();
            ensure!(
                !paste,
                "This clipboard format cannot be pasted into Android"
            );
            return Ok(());
        }
        state.unsupported_host = false;
        content["origin"] = json!(format!(
            "glidepod:{}:{}",
            state.epoch,
            host.unwrap_or_default()
        ));
        content["expected"] = json!(version);
        if !current(lease) {
            return Ok(());
        }
        let result = client.set_clipboard_guarded(content.clone(), || {
            current(lease)
                && command(json!({"op":"clipboard_status"}))
                    .is_ok_and(|v| v["revision"].as_i64() == host)
        });
        result?;
        state.host = host;
        let after = client.clipboard()?;
        if after["origin"] == content["origin"] || after["kind"] == "empty" {
            state.guest = after["version"].as_str().unwrap_or("").into();
        }
    } else if state.guest != version {
        if guest["kind"] == "unavailable" {
            state.guest = version.into();
            return Ok(());
        }
        let mut content = guest.clone();
        if content["kind"] == "files" || content["kind"] == "image" {
            content["paths"] = json!(client.materialize_clipboard(&content)?);
        }
        if !current(lease) || client.clipboard()?["version"] != guest["version"] {
            return Ok(());
        }
        content["op"] = json!("clipboard_write");
        content["expected"] = json!(host);
        content["origin"] = json!(format!("android:{}:{version}", state.epoch));
        let reply = command(content)?;
        if reply["stale"] != true {
            state.unsupported_host = false;
            state.host = reply["revision"].as_i64();
            state.guest = version.into();
            client.retain_native_clipboard(
                (guest["kind"] == "files" || guest["kind"] == "image").then_some(version),
            )?;
        }
    }
    if paste {
        ensure!(current(lease), "Window focus changed before paste");
    }
    Ok(())
}

enum Job {
    Complete(PickerRequest, Vec<PathBuf>, bool),
    Drop(Vec<PathBuf>),
    Paste(u64),
}
#[derive(Default)]
struct Pending {
    requests: Vec<PickerRequest>,
    error: Option<String>,
    latest: Option<PathBuf>,
    pastes: VecDeque<u64>,
    retry: Option<PickerRequest>,
    busy: Option<&'static str>,
    completed: bool,
    background_error: Option<String>,
}
impl Pending {
    fn failed(&mut self, error: String, retry: Option<PickerRequest>) {
        self.error = Some(error);
        // A later paste/drop failure must not erase the earlier picker that
        // still needs Choose Again or Cancel before the UI has consumed it.
        if retry.is_some() {
            self.retry = retry;
        }
    }
}
pub struct HostFiles {
    send: mpsc::SyncSender<Job>,
    pending: Arc<Mutex<Pending>>,
    stopped: Arc<AtomicBool>,
    lease: Arc<AtomicU64>,
    opened: HashMap<String, PickerRequest>,
    last: Instant,
    completing: HashSet<String>,
    deferred: Option<Job>,
    pub latest: Option<PathBuf>,
    pub error: Option<String>,
    pub background_error: Option<String>,
    pub busy: Option<&'static str>,
    retry: Option<PickerRequest>,
}
impl HostFiles {
    pub fn new(window: Arc<AppWindow>, stopped: Arc<AtomicBool>) -> Self {
        let (send, receive) = mpsc::sync_channel(8);
        let pending = Arc::new(Mutex::new(Pending::default()));
        let output = pending.clone();
        let lease = Arc::new(AtomicU64::new(0));
        let focus = lease.clone();
        let done = stopped.clone();
        // Bulk IO must not retain the Android window's task/display lease after
        // its native window closes. Guest identity alone is enough for transfers.
        let guest = window.guest.clone();
        let info = window.info.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                let mut background_error = None;
                let result = (|| -> Result<()> {
                    match receive.recv_timeout(Duration::from_millis(350)) {
                        Ok(job) => {
                            let retry = match &job {
                                Job::Complete(r, _, false) => Some(r.clone()),
                                _ => None,
                            };
                            output.lock().unwrap().busy = Some(match &job {
                                Job::Complete(_, _, true) => "Cancelling…",
                                Job::Complete(_, _, false) | Job::Drop(_) => {
                                    "Sending files to Android…"
                                }
                                Job::Paste(_) => "Preparing paste…",
                            });
                            let transferred = (|| -> Result<()> {
                                match job {
                                    Job::Complete(r, paths, cancel) => {
                                        guest.complete_picker(&r, &paths, cancel)?
                                    }
                                    Job::Drop(paths) => guest.deliver_files(
                                        &info.package_id,
                                        info.display_id,
                                        &paths,
                                    )?,
                                    Job::Paste(lease) => {
                                        synchronize_clipboard(&guest, lease, true)?;
                                        if current(lease) {
                                            let mut p = output.lock().unwrap();
                                            ensure!(
                                                p.pastes.len() < 64,
                                                "Too many paste operations are waiting"
                                            );
                                            p.pastes.push_back(lease);
                                        }
                                    }
                                }
                                Ok(())
                            })();
                            let mut p = output.lock().unwrap();
                            p.busy = None;
                            match transferred {
                                Ok(()) => {
                                    p.completed = true;
                                    if p.retry.is_none() {
                                        p.error = None;
                                    }
                                }
                                Err(e) => {
                                    p.failed(format!("Could not transfer: {e:#}"), retry);
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if done.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let events = guest.file_events()?;
                    output.lock().unwrap().requests = events
                        .requests
                        .into_iter()
                        .filter(|r| r.package == info.package_id && r.display == info.display_id)
                        .collect();
                    if let Some(status) = guest.export_status()? {
                        let mut pending = output.lock().unwrap();
                        pending.latest = status.recent.last().cloned();
                        if let Some(error) = status.error {
                            background_error = Some(format!("Download export is waiting: {error}"));
                        }
                    }
                    let active = focus.load(Ordering::Acquire);
                    if current(active) {
                        synchronize_clipboard(&guest, active, false)?;
                    }
                    Ok(())
                })();
                let failed = result.is_err();
                output.lock().unwrap().background_error = result
                    .err()
                    .map(|e| format!("File or clipboard transfer: {e:#}"))
                    .or(background_error);
                if failed {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        });
        Self {
            send,
            pending,
            stopped,
            lease,
            opened: HashMap::new(),
            completing: HashSet::new(),
            deferred: None,
            latest: None,
            error: None,
            background_error: None,
            busy: None,
            retry: None,
            last: Instant::now() - Duration::from_secs(1),
        }
    }
    pub fn due(&self) -> bool {
        self.last.elapsed() >= Duration::from_millis(250)
    }
    pub fn focus(&self, lease: u64) {
        if self.lease.swap(lease, Ordering::AcqRel) != lease {
            self.pending
                .lock()
                .unwrap()
                .pastes
                .retain(|pending| *pending == lease);
        }
    }
    pub fn paste(&self, lease: u64) -> Result<()> {
        self.send
            .try_send(Job::Paste(lease))
            .map_err(|_| anyhow::anyhow!("A file transfer is already in progress"))
    }
    pub fn take_paste(&self) -> Option<u64> {
        self.pending.lock().unwrap().pastes.pop_front()
    }
    pub fn can_retry(&self) -> bool {
        self.retry.is_some()
    }
    pub fn retry(&mut self) {
        if let Some(request) = self.retry.take() {
            self.completing.remove(&request.id);
            self.error = None;
        }
    }
    pub fn cancel_retry(&mut self) -> Result<()> {
        ensure!(
            self.deferred.is_none(),
            "A selection is queued; try Cancel again shortly"
        );
        if let Some(request) = self.retry.take() {
            self.deferred = Some(Job::Complete(request, Vec::new(), true));
            self.error = None;
        }
        Ok(())
    }
    pub fn drop_files(&self, paths: Vec<PathBuf>) -> Result<()> {
        ensure!(
            !paths.is_empty() && paths.len() <= 64,
            "Drop up to 64 files at a time"
        );
        self.send
            .try_send(Job::Drop(paths))
            .map_err(|_| anyhow::anyhow!("A file transfer is already in progress"))
    }
    pub fn poll(&mut self, window: &mut gpui::Window) -> Result<()> {
        if self.last.elapsed() < Duration::from_millis(250) {
            return Ok(());
        }
        self.last = Instant::now();
        let requests = {
            let mut p = self.pending.lock().unwrap();
            self.latest = p.latest.clone();
            self.busy = p.busy;
            self.background_error = p.background_error.clone();
            if std::mem::take(&mut p.completed) && self.retry.is_none() {
                self.error = None;
            }
            if let Some(error) = p.error.take() {
                self.error = Some(error);
            }
            if let Some(request) = p.retry.take() {
                self.retry = Some(request);
            }
            p.requests.clone()
        };
        if self
            .retry
            .as_ref()
            .is_some_and(|r| !requests.iter().any(|current| current.id == r.id))
        {
            self.retry = None;
        }
        if let Some(job) = self.deferred.take() {
            match self.send.try_send(job) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(job)) => {
                    self.deferred = Some(job);
                    return Ok(());
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    anyhow::bail!("File transfer worker stopped")
                }
            }
        }
        for id in self.opened.keys().cloned().collect::<Vec<_>>() {
            if !requests.iter().any(|r| r.id == id) {
                command(json!({"op":"cancel_picker","id":id}))?;
                self.opened.remove(&id);
                continue;
            }
            let result = command(json!({"op":"picker_result","id":id}))?;
            if result["ready"] == true {
                let paths: Vec<PathBuf> = serde_json::from_value(result["paths"].clone())?;
                let job = Job::Complete(self.opened[&id].clone(), paths, result["cancel"] == true);
                self.completing.insert(id.clone());
                self.opened.remove(&id);
                match self.send.try_send(job) {
                    Ok(()) => {}
                    // The native result is consumed exactly once. Retain it until
                    // the bounded worker queue has room rather than losing a save.
                    Err(mpsc::TrySendError::Full(job)) => self.deferred = Some(job),
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        anyhow::bail!("File transfer worker stopped")
                    }
                }
                // Don't reopen the same request while its result is being transferred.
                return Ok(());
            }
        }
        self.completing
            .retain(|id| requests.iter().any(|r| &r.id == id));
        if self.opened.is_empty()
            && self.retry.is_none()
            && let Some(r) = requests.iter().find(|r| !self.completing.contains(&r.id))
        {
            let handle = HasWindowHandle::window_handle(window)
                .map_err(|_| anyhow::anyhow!("Mac app window is unavailable"))?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                anyhow::bail!("Mac app window is unavailable")
            };
            let mut request = serde_json::to_value(r)?;
            request["op"] = json!("pick");
            request["view"] = json!(handle.ns_view.as_ptr() as usize);
            command(request)?;
            self.opened.insert(r.id.clone(), r.clone());
            self.error = None;
        }
        Ok(())
    }
}
impl Drop for HostFiles {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.focus(0);
        for r in self.opened.values() {
            let _ = command(json!({"op":"cancel_picker","id":r.id}));
            let _ = self
                .send
                .try_send(Job::Complete(r.clone(), Vec::new(), true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn later_transfer_failure_preserves_pending_picker_retry() {
        let mut pending = Pending::default();
        let request = PickerRequest {
            id: "00000000-0000-4000-8000-000000000001".into(),
            package: "dev.glidepod.filefixture".into(),
            display: 1,
            task: 1,
            kind: "open".into(),
            mime_types: vec!["text/plain".into()],
            maximum: 1,
            name: "test".into(),
        };
        pending.failed("Picker transfer failed".into(), Some(request.clone()));
        pending.failed("Paste failed before the next UI tick".into(), None);
        assert_eq!(pending.retry.as_ref().map(|r| &r.id), Some(&request.id));
        assert!(pending.error.as_ref().unwrap().contains("Paste"));
    }
    #[test]
    #[ignore = "requires a disposable running GLIDEPOD_TEST_STORE and the original file fixture"]
    fn real_file_worker_does_not_retain_closed_window() -> Result<()> {
        use anyhow::Context;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let data =
            PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing test store")?)
                .canonicalize()?;
        ensure!(
            data.starts_with(root.join(".cache")),
            "Use a disposable store"
        );
        let guest = glidepod_runtime::Store::new(data)?.guest()?;
        let window = Arc::new(guest.open_app("dev.glidepod.filefixture", 720, 1080)?);
        let weak = Arc::downgrade(&window);
        let mut files = HostFiles::new(window.clone(), Arc::new(AtomicBool::new(false)));
        // Closing must release the app's task/display even while its file worker
        // still exists. Retaining the Arc formerly kept Android alive invisibly.
        drop(window);
        ensure!(
            weak.upgrade().is_none(),
            "File worker retained the closed Android window"
        );
        // Model rapid acknowledgements before the next native UI tick. Every
        // paste is delivered; changing focus removes only obsolete commands.
        files.pending.lock().unwrap().pastes.extend([11, 11, 11]);
        ensure!(
            files.take_paste() == Some(11)
                && files.take_paste() == Some(11)
                && files.take_paste() == Some(11),
            "Rapid paste acknowledgements were coalesced"
        );
        files.pending.lock().unwrap().pastes.extend([11, 12, 11]);
        files.focus(12);
        ensure!(
            files.take_paste() == Some(12) && files.take_paste().is_none(),
            "Obsolete paste survived focus transfer"
        );
        // Cancelling an older failed picker must not overwrite a queued result.
        let request = PickerRequest {
            id: "00000000-0000-4000-8000-000000000001".into(),
            package: "dev.glidepod.filefixture".into(),
            display: 1,
            task: 1,
            kind: "open".into(),
            mime_types: vec!["text/plain".into()],
            maximum: 1,
            name: "test".into(),
        };
        files.retry = Some(request.clone());
        files.deferred = Some(Job::Complete(
            request,
            vec![PathBuf::from("selected.txt")],
            false,
        ));
        ensure!(
            files.cancel_retry().is_err() && files.retry.is_some() && files.deferred.is_some(),
            "Cancellation lost a queued selection"
        );
        drop(files);
        Ok(())
    }
}
