//! Content transfers and atomic, idempotent exports. No host directories are mounted.
use crate::{
    adb::{Adb, quote},
    experimental::{atomic_json, digest},
    guest::{Guest, package_id},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

const AUTHORITY: &str = "dev.glidepod.files.bridge";
const CLASS: &str = "dev.glidepod.bridge.Files";
pub const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub revision: u64,
    #[serde(default)]
    pub request: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PickerRequest {
    pub id: String,
    pub package: String,
    pub display: u32,
    pub task: u32,
    pub kind: String,
    pub mime_types: Vec<String>,
    pub maximum: usize,
    pub name: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct FileEvents {
    pub requests: Vec<PickerRequest>,
    pub exports: Vec<Document>,
    pub system: bool,
    #[serde(default)]
    pub errors: Vec<String>,
}
#[derive(Deserialize, Default)]
pub struct ExportStatus {
    pub epoch: u64,
    pub recent: Vec<PathBuf>,
    pub error: Option<String>,
}
#[derive(Deserialize)]
struct Download {
    key: String,
    name: String,
    size: u64,
}
#[derive(Deserialize)]
struct Snapshot {
    path: String,
    size: u64,
    sha256: String,
}
struct ImportedBatch<'a> {
    guest: &'a Guest,
    ids: Vec<String>,
    delivered: bool,
}
impl Drop for ImportedBatch<'_> {
    fn drop(&mut self) {
        if !self.delivered {
            self.guest.discard_imports(&self.ids);
        }
    }
}
fn uuid(value: &str) -> Result<()> {
    ensure!(
        value.len() == 36
            && value
                .bytes()
                .enumerate()
                .all(|(i, b)| if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }),
        "Invalid file request identity"
    );
    Ok(())
}
fn sha(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid file checksum"
    );
    Ok(())
}
fn key(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn accepts_mime(types: &[String], mime: &str) -> bool {
    types.iter().any(|t| {
        t == "*/*"
            || t.eq_ignore_ascii_case(mime)
            || t.strip_suffix("/*").is_some_and(|prefix| {
                mime.split_once('/')
                    .is_some_and(|(major, _)| major.eq_ignore_ascii_case(prefix))
            })
    })
}
fn safe_name(name: &str) -> String {
    let mut clean: String = name
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '_'
            } else {
                c
            }
        })
        .take(160)
        .collect();
    while clean.starts_with('.') {
        clean.remove(0);
    }
    while clean.len() > 200 {
        clean.pop();
    }
    if clean.trim().is_empty() {
        "Download".into()
    } else {
        clean
    }
}
fn valid_leaf(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 255
        && !name.contains(['/', '\0'])
}
fn numbered_name(original: &str, index: usize) -> String {
    let path = Path::new(original);
    let suffix = format!(" ({index})");
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .filter(|s| s.len() < 200)
        .map(|s| format!(".{s}"))
        .unwrap_or_default();
    let stem = if extension.is_empty() {
        original
    } else {
        path.file_stem().unwrap().to_str().unwrap()
    };
    let mut stem = stem.to_owned();
    while stem.len() + suffix.len() + extension.len() > 255 {
        stem.pop();
    }
    format!("{stem}{suffix}{extension}")
}
impl Guest {
    pub fn export_status(&self) -> Result<Option<ExportStatus>> {
        let path = self.store.root().join("file-export-status.json");
        if !path.exists() {
            return Ok(None);
        }
        let file = File::open(path)?;
        ensure!(file.metadata()?.len() <= 65536, "Invalid export status");
        let status: ExportStatus = serde_json::from_reader(file)?;
        ensure!(status.recent.len() <= 8, "Invalid export history");
        Ok((status.epoch == self.endpoints.epoch).then_some(status))
    }
    pub fn clipboard(&self) -> Result<serde_json::Value> {
        let value: serde_json::Value =
            serde_json::from_str(&self.provider("dev.glidepod.input.clipboard", "read", None)?)?;
        ensure!(
            value["version"].as_str().is_some_and(|s| s.len() < 128),
            "Invalid clipboard revision"
        );
        ensure!(
            ["text", "empty", "image", "files", "unavailable"]
                .contains(&value["kind"].as_str().unwrap_or("")),
            "Unsupported clipboard data"
        );
        if value["kind"] == "text" {
            ensure!(
                value["text"].as_str().is_some_and(|s| s.len() <= 262144),
                "Clipboard text exceeds limit"
            );
        }
        if value["kind"] == "files" || value["kind"] == "image" {
            ensure!(
                value["items"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty() && v.len() <= 64),
                "Invalid clipboard files"
            );
        }
        Ok(value)
    }
    pub fn set_clipboard(&self, value: serde_json::Value) -> Result<()> {
        self.set_clipboard_guarded(value, || true)
    }
    pub fn set_clipboard_guarded(
        &self,
        mut value: serde_json::Value,
        still_current: impl Fn() -> bool,
    ) -> Result<()> {
        let kind = value["kind"].as_str().unwrap_or("").to_owned();
        ensure!(
            ["text", "empty", "image", "files"].contains(&kind.as_str()),
            "Unsupported clipboard format"
        );
        let mut batch = ImportedBatch {
            guest: self,
            ids: Vec::new(),
            delivered: false,
        };
        if kind == "files" || kind == "image" {
            let paths = value["paths"]
                .as_array()
                .context("Missing clipboard files")?;
            ensure!(
                !paths.is_empty() && paths.len() <= 64,
                "Clipboard has too many files"
            );
            for path in paths {
                batch.ids.push(
                    self.import_kind(
                        Path::new(path.as_str().context("Invalid clipboard path")?),
                        "clipboard",
                    )?
                    .id,
                );
            }
            let grants: serde_json::Value =
                self.file_call("clipboard", json!({"ids":batch.ids}))?;
            value["uris"] = grants["uris"].clone();
            value.as_object_mut().unwrap().remove("paths");
        }
        let data = serde_json::to_vec(&value)?;
        ensure!(data.len() <= 524288, "Clipboard exceeds transfer limit");
        let mut file = tempfile::NamedTempFile::new_in(self.store.root())?;
        file.write_all(&data)?;
        file.flush()?;
        let remote = format!(
            "/data/local/tmp/glidepod/clipboard-{}.json",
            key(&String::from_utf8(data)?)
        );
        let result = (|| {
            Adb::push(&self.endpoints.vsock, file.path(), &remote)?;
            ensure!(
                still_current(),
                "Clipboard or window focus changed during transfer"
            );
            if !batch.ids.is_empty() {
                let _: serde_json::Value = self.file_call("retain", json!({"ids":batch.ids}))?;
            }
            // A transport error can lose a reply after Android accepted the URI.
            // Preserve offered snapshots until a confirmed later clipboard update.
            batch.delivered = true;
            self.bridge(CLASS, &["clipboard-set", &remote])?;
            let _: serde_json::Value = self.file_call("clipboard_pin", json!({"ids":batch.ids}))?;
            Ok(())
        })();
        let _ = self.shell(&format!("rm -f {}", quote(&remote)));
        result
    }
    pub fn materialize_clipboard(&self, value: &serde_json::Value) -> Result<Vec<PathBuf>> {
        let version = value["version"]
            .as_str()
            .context("Missing clipboard version")?;
        let directory = self.store.root().join("clipboard").join(key(version));
        fs::create_dir_all(&directory)?;
        let pinned = self.native_clipboard_pin()?;
        // Keep the active version and allow one hour for asynchronous consumers
        // of superseded file URLs. Persistent documents are stored separately.
        for entry in fs::read_dir(directory.parent().unwrap())?.take(4096) {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if path != directory
                && pinned.as_deref() != entry.file_name().to_str()
                && metadata.is_dir()
                && sha(&entry.file_name().to_string_lossy()).is_ok()
                && metadata
                    .modified()?
                    .elapsed()
                    .is_ok_and(|age| age > Duration::from_secs(3600))
            {
                fs::remove_dir_all(path)?;
            }
        }
        let items = value["items"]
            .as_array()
            .context("Missing clipboard items")?;
        ensure!(items.len() <= 64, "Too many clipboard items");
        let mut paths = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let name = safe_name(item["name"].as_str().unwrap_or("Clipboard file"));
            // Keep the original filename on the pasteboard, including when two
            // clipboard items happen to share a name.
            let item_directory = directory.join(i.to_string());
            fs::create_dir_all(&item_directory)?;
            let path = item_directory.join(&name);
            if !path.exists() {
                let output = self.bridge_timeout(
                    CLASS,
                    &["clipboard-item", version, &i.to_string()],
                    Duration::from_secs(1800),
                )?;
                let snapshot: Snapshot = serde_json::from_str(&output)?;
                let expected = snapshot.sha256.clone();
                let mut staged = tempfile::NamedTempFile::new_in(&item_directory)?;
                self.snapshot_to(snapshot, staged.as_file_mut())?;
                ensure!(
                    digest(staged.path())? == expected,
                    "Clipboard file checksum mismatch"
                );
                staged.persist_noclobber(&path)?;
            }
            paths.push(path);
        }
        Ok(paths)
    }
    fn native_clipboard_pin(&self) -> Result<Option<String>> {
        let path = self.store.root().join("clipboard/current.json");
        match File::open(path) {
            Ok(file) => {
                ensure!(
                    file.metadata()?.len() <= 256,
                    "Invalid clipboard retention record"
                );
                let pin: Option<String> = serde_json::from_reader(file)?;
                if let Some(id) = &pin {
                    sha(id)?;
                }
                Ok(pin)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    /// Called only after the native pasteboard has accepted these file URLs.
    pub fn retain_native_clipboard(&self, version: Option<&str>) -> Result<()> {
        let directory = self.store.root().join("clipboard");
        fs::create_dir_all(&directory)?;
        let next = version.map(key);
        if let Some(previous) = self.native_clipboard_pin()?
            && Some(&previous) != next.as_ref()
        {
            let old = directory.join(previous);
            if old.is_dir() {
                File::open(old)?
                    .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
            }
        }
        atomic_json(&directory.join("current.json"), &next)
    }
    fn file_call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        arg: serde_json::Value,
    ) -> Result<T> {
        let input = serde_json::to_string(&arg)?;
        ensure!(input.len() <= 32768, "File request exceeds limit");
        Ok(serde_json::from_str(&self.provider(
            AUTHORITY,
            method,
            Some(&input),
        )?)?)
    }
    pub fn file_events(&self) -> Result<FileEvents> {
        let events: FileEvents = self.file_call("poll", json!({}))?;
        ensure!(
            events.system
                && events.requests.len() <= 16
                && events.exports.len() <= 64
                && events.errors.len() <= 8
                && events.errors.iter().all(|s| s.len() <= 4096),
            "Invalid file bridge response"
        );
        for r in &events.requests {
            uuid(&r.id)?;
            package_id(&r.package)?;
            ensure!(
                r.display > 0
                    && r.task > 0
                    && ["open", "save"].contains(&r.kind.as_str())
                    && (1..=64).contains(&r.maximum)
                    && !r.mime_types.is_empty()
                    && r.mime_types.len() <= 32
                    && r.mime_types
                        .iter()
                        .all(|t| t.len() <= 128 && t.contains('/'))
                    && r.name.len() <= 1024,
                "Invalid file picker request"
            );
        }
        for d in &events.exports {
            d.validate()?;
        }
        Ok(events)
    }
    pub fn import_file(&self, path: &Path) -> Result<Document> {
        self.import_kind(path, "import")
    }
    fn import_kind(&self, path: &Path, kind: &str) -> Result<Document> {
        self.verify_epoch()?;
        let mut source = File::open(path)?;
        let before = source.metadata()?;
        ensure!(
            source.metadata()?.is_file() && source.metadata()?.len() <= MAX_BYTES,
            "Select a regular file no larger than 64 GiB"
        );
        let mut staged = tempfile::NamedTempFile::new_in(self.store.root())?;
        let bytes = std::io::copy(&mut (&mut source).take(MAX_BYTES + 1), &mut staged)?;
        let after = source.metadata()?;
        ensure!(
            before.len() == after.len()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec(),
            "The selected file changed during transfer; select it again"
        );
        ensure!(bytes <= MAX_BYTES, "File exceeds 64 GiB");
        staged.flush()?;
        let name = path
            .file_name()
            .context("Missing filename")?
            .to_string_lossy();
        let mime = glidepod_macos::files::mime_type(path)?;
        let request = json!({"name":safe_name(&name),"mime":mime,"size":bytes,"sha256":digest(staged.path())?,"kind":kind});
        let document: Document = self.file_call("create", request.clone())?;
        uuid(&document.id)?;
        let remote = format!("/data/local/tmp/glidepod/import-{}.bin", document.id);
        let result = (|| {
            let mut file = File::open(staged.path())?;
            Adb::push_file(&self.endpoints.vsock, &mut file, &remote, MAX_BYTES)?;
            let output = self.bridge_timeout(
                CLASS,
                &["import", &document.id, &remote],
                Duration::from_secs(1800),
            )?;
            let committed: Document = serde_json::from_str(&output)?;
            committed.validate()?;
            ensure!(
                committed.size == bytes && committed.sha256 == request["sha256"],
                "Imported file digest mismatch"
            );
            Ok(committed)
        })();
        let _ = self.shell(&format!("rm -f {}", quote(&remote)));
        if result.is_err() {
            self.discard_imports(std::slice::from_ref(&document.id));
        }
        result
    }
    fn discard_imports(&self, ids: &[String]) {
        for id in ids {
            let _: Result<serde_json::Value> = self.file_call("discard", json!({"id":id}));
        }
    }
    pub fn complete_picker(
        &self,
        request: &PickerRequest,
        paths: &[PathBuf],
        cancel: bool,
    ) -> Result<()> {
        let current = self.file_events()?;
        ensure!(
            current.requests.iter().any(|r| r.id == request.id
                && r.package == request.package
                && r.display == request.display
                && r.task == request.task),
            "File selection expired"
        );
        if cancel {
            let _: serde_json::Value =
                self.file_call("result", json!({"request":request.id,"cancel":true}))?;
            return Ok(());
        }
        let mut ids = Vec::new();
        let mut selected_name = None;
        if request.kind == "save" {
            ensure!(paths.len() == 1, "Select one save destination");
            ensure!(
                paths[0].is_absolute()
                    && paths[0].parent().is_some_and(Path::is_dir)
                    && !paths[0].is_dir(),
                "Choose an available save destination"
            );
            selected_name = paths[0]
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned);
            ensure!(
                selected_name.as_deref().is_some_and(valid_leaf),
                "Choose a valid filename"
            );
            let directory = self.store.root().join("file-saves");
            fs::create_dir_all(&directory)?;
            uuid(&request.id)?;
            atomic_json(&directory.join(format!("{}.json", request.id)), &paths[0])?;
        } else {
            ensure!(
                !paths.is_empty() && paths.len() <= request.maximum,
                "Too many selected files"
            );
            // Validate the whole selection before copying its first byte, so an
            // unsupported item cannot leave half a batch in the provider.
            for path in paths {
                ensure!(
                    path.is_file(),
                    "Choose files, rather than folders, for this app"
                );
                let mime = glidepod_macos::files::mime_type(path)?;
                ensure!(
                    accepts_mime(&request.mime_types, &mime),
                    "{} is not a supported file type for this app",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            for path in paths {
                match self.import_file(path) {
                    Ok(doc) => ids.push(doc.id),
                    Err(e) => {
                        self.discard_imports(&ids);
                        return Err(e);
                    }
                }
            }
        }
        let result: Result<serde_json::Value> = self.file_call(
            "result",
            json!({"request":request.id,"ids":ids,"name":selected_name}),
        );
        if result.is_err() {
            self.discard_imports(&ids);
        }
        result?;
        Ok(())
    }
    pub fn deliver_files(&self, package: &str, display: u32, paths: &[PathBuf]) -> Result<()> {
        package_id(package)?;
        ensure!(
            display > 0 && !paths.is_empty() && paths.len() <= 64,
            "Invalid file delivery"
        );
        let count = || -> Result<()> {
            let count = self.bridge(
                "dev.glidepod.bridge.Tasks",
                &["count", &display.to_string(), package],
            )?;
            ensure!(
                serde_json::from_str::<serde_json::Value>(&count)?["count"]
                    .as_u64()
                    .unwrap_or(0)
                    > 0,
                "App no longer owns this window"
            );
            Ok(())
        };
        count()?;
        let mut mime = None;
        for path in paths {
            ensure!(
                path.is_file(),
                "Drag files into the app, rather than folders"
            );
            let next = glidepod_macos::files::mime_type(path)?;
            mime = Some(match mime {
                None => next,
                Some(previous) if previous == next => previous,
                _ => "*/*".into(),
            });
        }
        let supported: serde_json::Value = serde_json::from_str(&self.bridge(
            CLASS,
            &[
                "can-deliver",
                package,
                mime.as_deref().unwrap(),
                &paths.len().to_string(),
            ],
        )?)?;
        ensure!(
            supported["supported"] == true,
            "This app cannot receive these files by dropping. Use its file selection action"
        );
        let mut batch = ImportedBatch {
            guest: self,
            ids: Vec::new(),
            delivered: false,
        };
        for path in paths {
            batch.ids.push(self.import_file(path)?.id);
        }
        count()?; // A long transfer must not reopen an app the user just closed.
        let token: serde_json::Value = self.file_call(
            "deliver",
            json!({"package":package,"display":display,"ids":batch.ids}),
        )?;
        let token = token["token"].as_str().context("Missing delivery token")?;
        uuid(token)?;
        self.shell(&format!(
            "am start -W --display {display} -n dev.glidepod.files/.Deliver --es token {}",
            quote(token)
        ))?;
        batch.delivered = true;
        Ok(())
    }
    fn snapshot_to(&self, snapshot: Snapshot, out: &mut File) -> Result<()> {
        sha(&snapshot.sha256)?;
        ensure!(snapshot.size <= MAX_BYTES, "Export exceeds limit");
        ensure!(
            snapshot
                .path
                .strip_prefix("/data/local/tmp/glidepod/export-")
                .and_then(|s| s.strip_suffix(".bin"))
                .is_some_and(|s| uuid(s).is_ok()),
            "Invalid export staging path"
        );
        let result = Adb::pull(&self.endpoints.vsock, &snapshot.path, out, snapshot.size);
        // Validate again before using the guest-provided path in a command.
        if snapshot
            .path
            .strip_prefix("/data/local/tmp/glidepod/export-")
            .and_then(|s| s.strip_suffix(".bin"))
            .is_some_and(|s| uuid(s).is_ok())
        {
            let _ = self.shell(&format!("rm -f {}", quote(&snapshot.path)));
        }
        result?;
        out.sync_all()?;
        Ok(())
    }
}
impl Document {
    fn validate(&self) -> Result<()> {
        uuid(&self.id)?;
        sha(&self.sha256)?;
        ensure!(
            self.size <= MAX_BYTES && self.name.len() <= 1024 && self.mime.len() <= 128,
            "Invalid document metadata"
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    version: u32,
    complete: bool,
    name: String,
    partial: String,
    sha256: String,
    size: u64,
}
/// A prepared record plus a hard link makes publication recoverable without overwriting
/// an existing destination or recreating a completed export that the user deleted.
pub struct Exporter {
    guest: Guest,
    directory: PathBuf,
    journal: PathBuf,
}
pub(crate) struct ExportWorker(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl ExportWorker {
    pub(crate) fn start(guest: Guest) -> Self {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        std::thread::spawn(move || {
            let mut exporter = None;
            let mut recent = Vec::<PathBuf>::new();
            let mut published = None;
            while !stop.load(Ordering::Acquire) {
                let result = (|| -> Result<()> {
                    guest.verify_epoch()?;
                    if exporter.is_none() {
                        exporter = Some(Exporter::new(
                            guest.clone(),
                            glidepod_macos::files::downloads_directory()?,
                        )?);
                    }
                    let paths = exporter.as_ref().unwrap().cycle()?;
                    recent.extend(paths);
                    if recent.len() > 8 {
                        recent.drain(..recent.len() - 8);
                    }
                    Ok(())
                })();
                let pending_errors: Vec<String> =
                    fs::read(guest.store.root().join("file-exports/errors.json"))
                        .ok()
                        .filter(|b| b.len() < 32768)
                        .and_then(|b| serde_json::from_slice(&b).ok())
                        .unwrap_or_default();
                let status = json!({"epoch":guest.endpoints.epoch,"recent":recent,"error":result.err().map(|e|e.to_string()).or_else(||pending_errors.first().cloned())});
                // Status is a snapshot, not a heartbeat. Avoid an fsync and rename
                // every idle cycle; retain retries if a changed snapshot cannot be saved.
                if published.as_ref() != Some(&status)
                    && atomic_json(&guest.store.root().join("file-export-status.json"), &status)
                        .is_ok()
                {
                    published = Some(status);
                }
                for _ in 0..30 {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self(stopped)
    }
}
impl Drop for ExportWorker {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}
impl Exporter {
    pub fn new(guest: Guest, directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        let journal = guest.store.root().join("file-exports");
        fs::create_dir_all(&journal)?;
        Ok(Self {
            guest,
            directory,
            journal,
        })
    }
    fn record(&self, id: &str, receipt: &Receipt) -> Result<()> {
        atomic_json(&self.journal.join(format!("{id}.json")), receipt)?;
        File::open(&self.journal)?.sync_all()?;
        Ok(())
    }
    fn export(
        &self,
        identity: &str,
        name: &str,
        target: Option<&Path>,
        produce: impl FnOnce(&mut File) -> Result<(u64, String)>,
    ) -> Result<Option<PathBuf>> {
        let id = key(identity);
        let record_path = self.journal.join(format!("{id}.json"));
        let destination = target.and_then(Path::parent).unwrap_or(&self.directory);
        ensure!(
            destination.is_dir(),
            "Mac export destination is unavailable"
        );
        let original_name = if let Some(target) = target {
            let name = target
                .file_name()
                .and_then(|s| s.to_str())
                .context("Invalid save filename")?;
            ensure!(valid_leaf(name), "Invalid save filename");
            name.to_owned()
        } else {
            safe_name(name)
        };
        let mut receipt = if record_path.exists() {
            let f = File::open(&record_path)?;
            ensure!(f.metadata()?.len() < 8192, "Invalid export journal");
            let r: Receipt = serde_json::from_reader(f)?;
            ensure!(
                r.version == 1
                    && valid_leaf(&r.name)
                    && r.partial == format!(".glidepod-{id}.partial"),
                "Invalid export receipt"
            );
            if r.complete {
                // A crash after the durable receipt but before unlinking leaves
                // only this owned staging link. Never republish the user copy.
                let leftover = destination.join(&r.partial);
                if fs::symlink_metadata(&leftover).is_ok_and(|m| m.is_file()) {
                    fs::remove_file(leftover)?;
                    File::open(destination)?.sync_all()?;
                }
                return Ok(None);
            }
            r
        } else {
            Receipt {
                version: 1,
                complete: false,
                name: original_name.clone(),
                partial: format!(".glidepod-{id}.partial"),
                sha256: String::new(),
                size: 0,
            }
        };
        let partial = destination.join(&receipt.partial);
        if receipt.sha256.is_empty() {
            // No prepared receipt means an interrupted transfer, never a published file.
            if partial.exists() {
                fs::remove_file(&partial)?;
            }
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&partial)?;
            let result = produce(&mut output);
            let (size, hash) = match result {
                Ok(v) => v,
                Err(e) => {
                    let _ = fs::remove_file(&partial);
                    return Err(e);
                }
            };
            output.sync_all()?;
            sha(&hash)?;
            ensure!(
                output.metadata()?.len() == size && digest(&partial)? == hash,
                "Export checksum mismatch"
            );
            receipt.size = size;
            receipt.sha256 = hash;
            self.record(&id, &receipt)?;
        }
        ensure!(partial.is_file(), "Export recovery snapshot is missing");
        let source = fs::symlink_metadata(&partial)?;
        ensure!(source.is_file(), "Invalid export recovery snapshot");
        ensure!(
            source.len() == receipt.size && digest(&partial)? == receipt.sha256,
            "Export recovery checksum mismatch"
        );
        let mut index = 1;
        let final_path = loop {
            let path = destination.join(&receipt.name);
            if let Ok(existing) = fs::symlink_metadata(&path) {
                if existing.is_file()
                    && existing.ino() == source.ino()
                    && existing.dev() == source.dev()
                {
                    break path;
                }
                index += 1;
                receipt.name = numbered_name(&original_name, index);
                self.record(&id, &receipt)?;
                continue;
            }
            match fs::hard_link(&partial, &path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
        File::open(destination)?.sync_all()?;
        receipt.complete = true;
        self.record(&id, &receipt)?;
        fs::remove_file(partial)?;
        File::open(destination)?.sync_all()?;
        Ok(Some(final_path))
    }
    pub fn cycle(&self) -> Result<Vec<PathBuf>> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(self.journal.join(".lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("Another download export is already running")?;
        self.guest.verify_epoch()?;
        let mut saved = Vec::new();
        let mut failures = Vec::new();
        let events = self.guest.file_events()?;
        failures.extend(events.errors);
        for doc in events.exports {
            let result = (|| -> Result<Option<PathBuf>> {
                let identity = format!("document:{}:{}", doc.id, doc.revision);
                let target = if let Some(request) = &doc.request {
                    uuid(request)?;
                    let f = File::open(
                        self.guest
                            .store
                            .root()
                            .join("file-saves")
                            .join(format!("{request}.json")),
                    )?;
                    ensure!(f.metadata()?.len() < 8192, "Invalid save destination");
                    Some(serde_json::from_reader::<_, PathBuf>(f)?)
                } else {
                    None
                };
                let path = self.export(&identity, &doc.name, target.as_deref(), |file| {
                    let output = self.guest.bridge_timeout(
                        CLASS,
                        &["export", &doc.id, &doc.revision.to_string()],
                        Duration::from_secs(1800),
                    )?;
                    let snapshot: Snapshot = serde_json::from_str(&output)?;
                    ensure!(
                        snapshot.sha256 == doc.sha256 && snapshot.size == doc.size,
                        "Saved document changed"
                    );
                    self.guest.snapshot_to(snapshot, file)?;
                    Ok((doc.size, doc.sha256.clone()))
                })?;
                let _: serde_json::Value = self
                    .guest
                    .file_call("ack", json!({"id":doc.id,"revision":doc.revision}))?;
                Ok(path)
            })();
            match result {
                Ok(Some(path)) => saved.push(path),
                Ok(None) => {}
                Err(e) => {
                    if failures.len() < 8 {
                        failures.push(e.to_string());
                    }
                }
            }
        }
        #[derive(Deserialize)]
        struct Downloads {
            items: Vec<Download>,
        }
        let output = self.guest.bridge(CLASS, &["downloads"])?;
        let list: Downloads = serde_json::from_str(&output)?;
        ensure!(list.items.len() <= 4096, "Too many download entries");
        let baseline = self.journal.join("baseline.json");
        let ignored: HashSet<String> = if baseline.exists() {
            let f = File::open(&baseline)?;
            ensure!(
                f.metadata()?.len() < 512 * 1024,
                "Invalid download baseline"
            );
            serde_json::from_reader(f)?
        } else {
            let keys: HashSet<_> = list.items.iter().map(|d| d.key.clone()).collect();
            atomic_json(&baseline, &keys)?;
            keys
        };
        for d in list.items {
            sha(&d.key)?;
            ensure!(
                d.size <= MAX_BYTES && d.name.len() <= 1024,
                "Invalid download"
            );
            if ignored.contains(&d.key) {
                continue;
            }
            let result = self.export(&format!("download:{}", d.key), &d.name, None, |file| {
                let output = self.guest.bridge_timeout(
                    CLASS,
                    &["download", &d.key],
                    Duration::from_secs(1800),
                )?;
                let snapshot: Snapshot = serde_json::from_str(&output)?;
                let size = snapshot.size;
                let hash = snapshot.sha256.clone();
                self.guest.snapshot_to(snapshot, file)?;
                Ok((size, hash))
            });
            match result {
                Ok(Some(path)) => saved.push(path),
                Ok(None) => {}
                Err(e) => {
                    if failures.len() < 8 {
                        failures.push(e.to_string());
                    }
                }
            }
        }
        atomic_json(&self.journal.join("errors.json"), &failures)?;
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_identifiers_cannot_escape_the_destination() {
        assert_eq!(safe_name("../../a\0/b.pdf"), "_.._a_b.pdf");
        assert_eq!(safe_name(".."), "Download");
        assert!(uuid("../../../../../../../../etc/passwd").is_err());
        assert!(uuid("bd0a427d-5016-4f05-9138-14a9337c124e").is_ok());
        assert!(safe_name(&"文".repeat(500)).len() <= 200);
    }
    fn test_exporter() -> Result<(tempfile::TempDir, Exporter)> {
        let tmp = tempfile::tempdir()?;
        let store = crate::Store::new(tmp.path().join("store"))?;
        let guest = Guest {
            store,
            endpoints: crate::guest::Endpoints {
                epoch: 1,
                vsock: PathBuf::new(),
                control: PathBuf::new(),
                frames: PathBuf::new(),
            },
        };
        let exporter = Exporter::new(guest, tmp.path().join("Downloads"))?;
        Ok((tmp, exporter))
    }
    fn bytes(out: &mut File) -> Result<(u64, String)> {
        out.write_all(b"complete data")?;
        Ok((13, key("complete data")))
    }
    #[test]
    fn publication_preserves_collisions_and_does_not_recreate_deleted_copies() -> Result<()> {
        let (_tmp, e) = test_exporter()?;
        fs::write(e.directory.join("report.txt"), "user file")?;
        let path = e.export("source-1", "report.txt", None, bytes)?.unwrap();
        assert_eq!(path.file_name().unwrap(), "report (2).txt");
        assert_eq!(
            fs::read_to_string(e.directory.join("report.txt"))?,
            "user file"
        );
        fs::remove_file(path)?;
        assert!(
            e.export("source-1", "report.txt", None, |_| panic!(
                "Completed transfer must not run again"
            ))?
            .is_none()
        );
        Ok(())
    }
    #[test]
    fn selected_hidden_names_are_preserved_and_long_collisions_fit_the_filesystem() -> Result<()> {
        let (_tmp, e) = test_exporter()?;
        let hidden = e.directory.join(".env");
        assert_eq!(
            e.export("hidden", "Untitled", Some(&hidden), bytes)?,
            Some(hidden)
        );
        let name = format!("{}.txt", "文".repeat(83));
        let target = e.directory.join(&name);
        fs::write(&target, "keep existing")?;
        let copy = e.export("long", "Untitled", Some(&target), bytes)?.unwrap();
        let actual = copy.file_name().unwrap().to_str().unwrap();
        assert!(actual.len() <= 255 && actual.ends_with(" (2).txt"));
        assert_eq!(fs::read_to_string(target)?, "keep existing");
        Ok(())
    }
    #[test]
    fn recovery_reconciles_published_inode_and_rejects_corrupt_partial() -> Result<()> {
        let (_tmp, e) = test_exporter()?;
        let id = key("crashed");
        let partial = format!(".glidepod-{id}.partial");
        fs::write(e.directory.join(&partial), b"complete data")?;
        let receipt = Receipt {
            version: 1,
            complete: false,
            name: "report.txt".into(),
            partial: partial.clone(),
            sha256: key("complete data"),
            size: 13,
        };
        e.record(&id, &receipt)?;
        fs::hard_link(e.directory.join(&partial), e.directory.join("report.txt"))?;
        assert_eq!(
            e.export("crashed", "report.txt", None, |_| panic!(
                "Recovery must use staged bytes"
            ))?,
            Some(e.directory.join("report.txt"))
        );
        assert_eq!(fs::read_dir(&e.directory)?.count(), 1);
        let id = key("corrupt");
        let partial = format!(".glidepod-{id}.partial");
        fs::write(e.directory.join(&partial), "wrong")?;
        e.record(
            &id,
            &Receipt {
                version: 1,
                complete: false,
                name: "bad.txt".into(),
                partial,
                sha256: key("complete data"),
                size: 13,
            },
        )?;
        assert!(e.export("corrupt", "bad.txt", None, bytes).is_err());
        assert!(!e.directory.join("bad.txt").exists());
        Ok(())
    }
    #[test]
    fn interrupted_transfer_has_no_visible_result_and_can_retry() -> Result<()> {
        let (_tmp, e) = test_exporter()?;
        assert!(
            e.export("interrupted", "data.txt", None, |out| {
                out.write_all(b"partial")?;
                anyhow::bail!("transport disconnected")
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(&e.directory)?.count(), 0);
        assert!(e.export("interrupted", "data.txt", None, bytes)?.is_some());
        Ok(())
    }
    #[test]
    fn completed_receipt_cleans_interrupted_unlink_without_touching_user_copy() -> Result<()> {
        let (_tmp, e) = test_exporter()?;
        let id = key("completed");
        let partial = format!(".glidepod-{id}.partial");
        let user_copy = e.directory.join("report.txt");
        fs::write(&user_copy, "user edits after publication")?;
        fs::hard_link(&user_copy, e.directory.join(&partial))?;
        e.record(
            &id,
            &Receipt {
                version: 1,
                complete: true,
                name: "report.txt".into(),
                partial,
                sha256: key("complete data"),
                size: 13,
            },
        )?;
        assert!(
            e.export("completed", "report.txt", None, |_| panic!(
                "Do not retransfer"
            ))?
            .is_none()
        );
        assert_eq!(
            fs::read_to_string(user_copy)?,
            "user edits after publication"
        );
        assert_eq!(fs::read_dir(&e.directory)?.count(), 1);
        Ok(())
    }
    #[test]
    fn clipboard_retention_starts_when_superseded_and_preserves_original_names() -> Result<()> {
        let (_tmp, exporter) = test_exporter()?;
        let guest = &exporter.guest;
        let root = guest.store.root().join("clipboard");
        let cached = |version: &str| -> Result<PathBuf> {
            let directory = root.join(key(version)).join("0");
            fs::create_dir_all(&directory)?;
            let path = directory.join("中文.txt");
            fs::write(&path, "clipboard content")?;
            Ok(path)
        };
        let first = cached("first")?;
        let second = cached("second")?;
        cached("third")?;
        let old_time = std::time::SystemTime::now() - Duration::from_secs(7200);
        let age = |version: &str| -> Result<()> {
            File::open(root.join(key(version)))?
                .set_times(fs::FileTimes::new().set_modified(old_time))?;
            Ok(())
        };
        guest.retain_native_clipboard(Some("first"))?;
        age("first")?;
        let value = |version| json!({"version":version,"items":[{"name":"中文.txt"}]});
        assert_eq!(guest.materialize_clipboard(&value("second"))?, vec![second]);
        assert!(
            first.exists(),
            "Current clipboard must stay pinned regardless of age"
        );
        guest.retain_native_clipboard(Some("second"))?;
        guest.materialize_clipboard(&value("third"))?;
        assert!(
            first.exists(),
            "Superseded clipboard needs a fresh grace period"
        );
        age("first")?;
        age("second")?;
        guest.materialize_clipboard(&value("third"))?;
        assert!(!first.exists());
        assert!(root.join(key("second")).exists());
        Ok(())
    }
    #[test]
    #[ignore = "requires the real supervisor in a disposable GLIDEPOD_TEST_STORE"]
    fn real_automatic_download_export() -> Result<()> {
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
        let guest = crate::Store::new(data)?.guest()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while guest.export_status()?.is_none() {
            ensure!(
                std::time::Instant::now() < deadline,
                "Supervisor did not start file exports"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        let name = format!(
            "GlidePod-acceptance-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let pending: serde_json::Value = serde_json::from_str(&guest.provider(
            "dev.glidepod.filefixture",
            "download",
            Some(&json!({"name":name,"text":"Automatic background export"}).to_string()),
        )?)?;
        guest.provider(
            "dev.glidepod.filefixture",
            "publish",
            Some(&pending.to_string()),
        )?;
        let deadline = std::time::Instant::now() + Duration::from_secs(45);
        let path = loop {
            if let Some(status) = guest.export_status()? {
                ensure!(
                    status.error.is_none(),
                    "Automatic exporter failed: {:?}",
                    status.error
                );
                if let Some(path) = status
                    .recent
                    .into_iter()
                    .find(|p| p.file_name().is_some_and(|n| n == name.as_str()))
                {
                    break path;
                }
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "Automatic export timed out"
            );
            std::thread::sleep(Duration::from_millis(200));
        };
        ensure!(
            path.parent() == Some(glidepod_macos::files::downloads_directory()?.as_path()),
            "Export did not use the native Downloads location"
        );
        ensure!(
            fs::read_to_string(&path)? == "Automatic background export",
            "Automatic export changed the contents"
        );
        fs::remove_file(&path)?;
        std::thread::sleep(Duration::from_secs(8));
        ensure!(
            !path.exists(),
            "Automatic exporter recreated a deleted copy"
        );
        let evidence = std::env::var_os("GLIDEPOD_TEST_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("evidence/host-files-2026-11-07"));
        fs::create_dir_all(&evidence)?;
        atomic_json(
            &evidence.join("automatic-export.json"),
            &json!({"passed":true,"epoch":guest.endpoints.epoch,"supervisor_owned":true,"default_downloads":true,"idempotent_after_deletion":true,"shared_mount":false}),
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "requires a disposable running GLIDEPOD_TEST_STORE"]
    fn real_rich_clipboard() -> Result<()> {
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
        let store = crate::Store::new(data)?;
        let guest = store.guest()?;
        guest.install(&root.join("target/android/glidepod-files.apk"), None)?;
        guest.prepare_input()?;
        let tmp = tempfile::tempdir_in(store.root())?;
        let source = tmp.path().join("clipboard.txt");
        fs::write(&source, "URI clipboard content")?;
        guest
            .set_clipboard(json!({"kind":"files","paths":[source],"origin":"file-test"}))
            .context("Set file clipboard")?;
        let clip = guest.clipboard()?;
        let paths = guest
            .materialize_clipboard(&clip)
            .context("Read file clipboard")?;
        ensure!(
            paths.len() == 1 && digest(&paths[0])? == digest(&source)?,
            "File clipboard bytes changed"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires a disposable running GLIDEPOD_TEST_STORE without the automatic exporter"]
    fn real_file_failure_recovery() -> Result<()> {
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
        let store = crate::Store::new(data)?;
        let guest = store.guest()?;
        guest.install(&root.join("target/android/glidepod-files.apk"), None)?;
        guest.install(&root.join("target/android/glidepod-file-fixture.apk"), None)?;
        let _window = guest.open_app("dev.glidepod.filefixture", 720, 1080)?;
        let fixture = |method: &str, arg: serde_json::Value| -> Result<serde_json::Value> {
            Ok(serde_json::from_str(&guest.provider(
                "dev.glidepod.filefixture",
                method,
                Some(&arg.to_string()),
            )?)?)
        };
        let wait = |check: &dyn Fn() -> Result<bool>| -> Result<()> {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while !check()? {
                ensure!(
                    std::time::Instant::now() < deadline,
                    "File recovery fixture timed out"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        };
        let choose = |action: &str, multiple: bool| -> Result<PickerRequest> {
            fixture(
                "choose",
                json!({"action":action,"multiple":multiple,"name":"Original.txt"}),
            )?;
            wait(&|| {
                Ok(guest
                    .file_events()?
                    .requests
                    .iter()
                    .any(|r| r.package == "dev.glidepod.filefixture"))
            })?;
            Ok(guest
                .file_events()?
                .requests
                .into_iter()
                .find(|r| r.package == "dev.glidepod.filefixture")
                .unwrap())
        };
        let docs = || -> Result<String> {
            guest.shell("su 0 find /data/user/0/dev.glidepod.files/files/documents -maxdepth 1 -name '*.json' | sort")
        };
        let temp = tempfile::tempdir_in(store.root())?;
        let corrupt = "/data/user/0/dev.glidepod.files/files/documents/00000000-0000-4000-8000-000000000099.json";
        guest.shell(&format!("su 0 test ! -e {}", quote(corrupt)))?;
        for invalid in ["not JSON", r#"{"ready":true}"#] {
            let source = temp.path().join("corrupt.json");
            fs::write(&source, invalid)?;
            let remote = "/data/local/tmp/glidepod/review-metadata.json";
            Adb::push(&guest.endpoints.vsock, &source, remote)?;
            guest.shell(&format!("su 0 cp {} {}", quote(remote), quote(corrupt)))?;
            let checked = (|| -> Result<()> {
                let events = guest.file_events()?;
                ensure!(
                    !events.errors.is_empty(),
                    "Corrupt metadata was not isolated and reported"
                );
                Ok(())
            })();
            guest.shell(&format!("su 0 rm -f {} {}", quote(corrupt), quote(remote)))?;
            checked?;
        }
        ensure!(
            guest.file_events()?.errors.is_empty(),
            "Metadata error did not clear after recovery"
        );
        let valid = temp.path().join("Valid.txt");
        fs::write(&valid, "First file")?;
        let huge = temp.path().join("TooLarge.txt");
        File::create(&huge)?.set_len(MAX_BYTES + 1)?;
        let request = choose("android.intent.action.GET_CONTENT", true)?;
        let before = docs()?;
        ensure!(
            guest
                .complete_picker(&request, &[valid.clone(), huge], false)
                .is_err(),
            "Oversized batch was accepted"
        );
        wait(&|| Ok(docs()? == before))?;
        ensure!(
            guest
                .file_events()?
                .requests
                .iter()
                .any(|r| r.id == request.id),
            "Failed selection cannot retry"
        );
        guest.complete_picker(&request, std::slice::from_ref(&valid), false)?;
        wait(&|| Ok(fixture("result", json!({}))?["code"] == -1))?;
        ensure!(
            guest
                .complete_picker(&request, std::slice::from_ref(&valid), false)
                .is_err(),
            "Obsolete picker result was accepted"
        );
        let before = docs()?;
        let clip = guest.clipboard()?;
        ensure!(
            guest
                .set_clipboard_guarded(
                    json!({"kind":"files","paths":[valid],"origin":"cancelled-review"}),
                    || false
                )
                .is_err(),
            "Cancelled clipboard upload was published"
        );
        wait(&|| Ok(docs()? == before))?;
        ensure!(
            guest.clipboard()?["version"] == clip["version"],
            "Cancelled transfer changed Android clipboard"
        );

        let save = choose("android.intent.action.CREATE_DOCUMENT", false)?;
        let destination = temp.path().join("Chosen name.txt");
        guest.complete_picker(&save, std::slice::from_ref(&destination), false)?;
        wait(&|| Ok(fixture("result", json!({}))?["code"] == -1))?;
        let result = fixture("result", json!({}))?;
        ensure!(
            result["items"][0]["name"] == "Chosen name.txt",
            "Save ignored the name chosen on Mac"
        );
        let uri = result["items"][0]["uri"]
            .as_str()
            .context("Missing saved URI")?;
        let id = uri.rsplit('/').next().unwrap();
        uuid(id)?;
        let metadata =
            || -> Result<serde_json::Value> { guest.file_call("metadata", json!({"id":id})) };
        let wait_revision = |revision| {
            wait(&|| {
                let m = metadata()?;
                Ok(m["revision"] == revision && m["writing"] == false)
            })
        };
        fixture("write", json!({"text":"Original bytes"}))?;
        wait_revision(1)?;
        fixture("write_at", json!({"mode":"rw","offset":9,"text":"DATA!"}))?;
        wait_revision(2)?;
        let expected = key("Original DATA!");
        ensure!(
            fixture("read_saved", json!({}))?["sha256"] == expected,
            "Seekable save changed unrelated bytes"
        );
        let dir = format!("/data/user/0/dev.glidepod.files/files/documents/{id}");
        let blocked = format!("{dir}.json.new");
        guest.shell(&format!(
            "su 0 sh -c {}",
            quote(&format!(
                "mkdir {}; touch {}/keep",
                quote(&blocked),
                quote(&blocked)
            ))
        ))?;
        let failed_commit = (|| -> Result<()> {
            fixture("write", json!({"text":"Uncommitted replacement"}))?;
            wait(&|| {
                Ok(guest
                    .shell(&format!(
                        "su 0 test -f {}",
                        quote(&format!("{dir}.rev-3.bin"))
                    ))
                    .is_ok()
                    && metadata()?["writing"] == false)
            })?;
            ensure!(
                metadata()?["revision"] == 2,
                "Failed metadata commit advanced the revision"
            );
            ensure!(
                fixture("read_saved", json!({}))?["sha256"] == expected,
                "Failed metadata commit destroyed the last saved content"
            );
            Ok(())
        })();
        guest.shell(&format!("su 0 rm -rf {}", quote(&blocked)))?;
        failed_commit?;
        fixture(
            "write_at",
            json!({"mode":"wa","offset":0,"text":" appended"}),
        )?;
        wait_revision(3)?;
        ensure!(
            fixture("read_saved", json!({}))?["sha256"] == key("Original DATA! appended"),
            "Append did not preserve the committed bytes"
        );
        ensure!(
            fixture("write_at", json!({"offset":MAX_BYTES,"text":"overflow"})).is_err(),
            "Oversized proxy write was accepted"
        );
        wait(&|| Ok(metadata()?["writing"] == false))?;
        ensure!(
            metadata()?["revision"] == 3,
            "Failed write published a partial revision"
        );
        ensure!(
            fixture("read_saved", json!({}))?["sha256"] == key("Original DATA! appended"),
            "Failed write damaged the saved document"
        );
        ensure!(
            !guest.file_events()?.errors.is_empty(),
            "Failed save was not surfaced"
        );
        fixture("write", json!({"text":"Recovered"}))?;
        wait_revision(4)?;
        ensure!(
            guest.file_events()?.errors.is_empty(),
            "Successful save left a stale failure"
        );
        let exporter = Exporter::new(guest.clone(), temp.path().join("exports"))?;
        ensure!(
            exporter.cycle()?.contains(&destination),
            "Recovered save was not exported"
        );
        ensure!(
            fs::read_to_string(&destination)? == "Recovered",
            "Recovered export changed content"
        );
        let out = std::env::var_os("GLIDEPOD_TEST_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("evidence/host-files-review-2026-11-07"));
        fs::create_dir_all(&out)?;
        atomic_json(
            &out.join("failure-recovery.json"),
            &json!({"passed":true,"epoch":guest.endpoints.epoch,"checks":["batch rollback and same-request retry","obsolete picker rejection","cancelled clipboard leaves no snapshots or clipboard change","chosen filename","seekable and append writes","injected metadata commit failure preserves previous bytes","oversized write rejection preserves previous revision","save error surfaced and cleared by retry","recovered export"]}),
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "requires a disposable running GLIDEPOD_TEST_STORE without the automatic exporter"]
    fn real_file_bridge_contracts() -> Result<()> {
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
        let store = crate::Store::new(data)?;
        let guest = store.guest()?;
        guest.install(&root.join("target/android/glidepod-files.apk"), None)?;
        guest.install(&root.join("target/android/glidepod-file-fixture.apk"), None)?;
        let mut window = guest.open_app("dev.glidepod.filefixture", 720, 1080)?;
        let fixture = |method: &str, arg: serde_json::Value| -> Result<serde_json::Value> {
            Ok(serde_json::from_str(&guest.provider(
                "dev.glidepod.filefixture",
                method,
                Some(&arg.to_string()),
            )?)?)
        };
        let wait =
            |test: &dyn Fn() -> Result<Option<serde_json::Value>>| -> Result<serde_json::Value> {
                let deadline = std::time::Instant::now() + Duration::from_secs(20);
                loop {
                    if let Some(v) = test()? {
                        return Ok(v);
                    }
                    ensure!(std::time::Instant::now() < deadline, "Fixture timed out");
                    std::thread::sleep(Duration::from_millis(100));
                }
            };
        let private = fixture("private", json!({}))?;
        ensure!(
            fixture("security", json!({}))?["denied"] == true,
            "Private transport is exposed to apps"
        );
        let tmp = tempfile::tempdir_in(store.root())?;
        let source = tmp.path().join("Unicode 中文.txt");
        fs::write(&source, "Host file contents · 中文\n")?;
        let source_hash = digest(&source)?;
        let request = |action: &str, multiple: bool| -> Result<PickerRequest> {
            fixture(
                "choose",
                json!({"action":action,"multiple":multiple,"name":"Saved.txt"}),
            )?;
            let v = wait(&|| {
                Ok(guest
                    .file_events()?
                    .requests
                    .into_iter()
                    .find(|r| r.package == "dev.glidepod.filefixture")
                    .map(|r| serde_json::to_value(r).unwrap()))
            })?;
            Ok(serde_json::from_value(v)?)
        };
        let result = || {
            wait(&|| {
                let v = fixture("result", json!({}))?;
                Ok(v.get("code").is_some().then_some(v))
            })
        };
        let open = request("android.intent.action.OPEN_DOCUMENT", false)?;
        guest.complete_picker(&open, std::slice::from_ref(&source), false)?;
        let imported = result()?;
        ensure!(
            imported["items"][0]["sha256"] == source_hash
                && imported["items"][0]["persistent"] == true,
            "OPEN_DOCUMENT content/grant mismatch: {imported}"
        );
        let delivered_id = imported["items"][0]["uri"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_owned();
        // The pending-delivery token itself must retain content before the
        // Android Activity runs. Losing an am-start acknowledgement must not
        // allow automatic host rollback to delete that queued delivery.
        let pending_doc = guest.import_file(&source)?;
        let _: serde_json::Value = guest.file_call(
            "deliver",
            json!({
                "package":"dev.glidepod.filefixture", "display":window.info.display_id,
                "ids":[pending_doc.id]
            }),
        )?;
        guest.discard_imports(std::slice::from_ref(&pending_doc.id));
        let retained: serde_json::Value =
            guest.file_call("metadata", json!({"id":pending_doc.id}))?;
        ensure!(
            retained["delivered"] == true && retained["sha256"] == source_hash,
            "Queued delivery was discarded before its Activity ran"
        );
        // Simulate cleanup after a lost transport acknowledgement. Once a URI
        // was offered to the app, automatic rollback must not revoke its data.
        guest.discard_imports(&[delivered_id]);
        ensure!(
            fixture("read_saved", json!({}))?["sha256"] == source_hash,
            "Uncertain acknowledgement cleanup deleted delivered content"
        );
        fs::write(&source, "Mac file edited independently")?;
        ensure!(
            fixture("read_saved", json!({}))?["sha256"] == source_hash,
            "Host edits changed an imported document"
        );
        drop(window);
        guest.shell("am force-stop dev.glidepod.filefixture")?;
        window = guest.open_app("dev.glidepod.filefixture", 720, 1080)?;
        ensure!(
            fixture("read_saved", json!({}))?["sha256"] == source_hash,
            "Persisted URI failed after app restart"
        );
        let cancel = request("android.intent.action.GET_CONTENT", false)?;
        guest.complete_picker(&cancel, &[], true)?;
        ensure!(
            result()?["code"] == 0,
            "Cancellation changed the app result"
        );
        let multiple = request("android.intent.action.GET_CONTENT", true)?;
        guest.complete_picker(&multiple, &[source.clone(), source.clone()], false)?;
        ensure!(
            result()?["items"].as_array().unwrap().len() == 2,
            "Multi-select lost an item"
        );
        let destination = tmp.path().join("exports");
        let exporter = Exporter::new(guest.clone(), destination.clone())?;
        exporter.cycle()?; // Capture existing download history before the fixture publishes.
        let save = request("android.intent.action.CREATE_DOCUMENT", false)?;
        let selected = destination.join("Saved.txt");
        guest.complete_picker(&save, std::slice::from_ref(&selected), false)?;
        ensure!(
            result()?["items"][0]["unwritten"] == true,
            "Save must return before the app writes bytes"
        );
        fixture("write", json!({"text":"Android saved content"}))?;
        wait(&|| Ok((!guest.file_events()?.exports.is_empty()).then_some(json!(true))))?;
        let saved = exporter.cycle()?;
        ensure!(
            saved.contains(&selected) && fs::read_to_string(&selected)? == "Android saved content",
            "Save export content mismatch"
        );
        ensure!(exporter.cycle()?.is_empty(), "Save export was duplicated");
        let pending = fixture(
            "download",
            json!({"name":"bridge-download.txt","text":"Published download"}),
        )?;
        ensure!(
            exporter.cycle()?.is_empty(),
            "Pending download exported before publication"
        );
        fixture("publish", pending)?;
        let downloads = exporter.cycle()?;
        ensure!(
            downloads.len() == 1 && fs::read_to_string(&downloads[0])? == "Published download",
            "Published download not exported once: {downloads:?}"
        );
        fs::remove_file(&downloads[0])?;
        ensure!(
            exporter.cycle()?.is_empty(),
            "Deleted host download was recreated"
        );
        let dm = fixture("download_manager", json!({"private":false}))?;
        let original = wait(&|| {
            let v = fixture("download_status", dm.clone())?;
            ensure!(v["status"] != 16, "DownloadManager failed: {v}");
            Ok((v["status"] == 8).then_some(v))
        })?;
        let public = exporter.cycle()?;
        ensure!(
            public.len() == 1 && digest(&public[0])? == original["sha256"],
            "DownloadManager export mismatch"
        );
        let private_download = fixture("download_manager", json!({"private":true}))?;
        wait(&|| {
            let v = fixture("download_status", private_download.clone())?;
            ensure!(v["status"] != 16, "Private fixture download failed: {v}");
            Ok((v["status"] == 8).then_some(v))
        })?;
        ensure!(
            exporter.cycle()?.is_empty(),
            "Private destination or duplicate download was exported"
        );
        fs::write(&public[0], "Mac edit")?;
        ensure!(
            fixture("download_status", dm.clone())?["sha256"] == original["sha256"],
            "Mac edits changed the Android original"
        );
        guest.deliver_files(
            "dev.glidepod.filefixture",
            window.info.display_id,
            std::slice::from_ref(&source),
        )?;
        let delivered = result()?;
        ensure!(
            delivered["items"][0]["sha256"] == digest(&source)?,
            "Targeted drop changed bytes"
        );
        let before = guest.clipboard()?;
        guest.set_clipboard(json!({"kind":"text","text":"Shared clipboard 中文","origin":"test-host","expected":before["version"]}))?;
        ensure!(
            guest.clipboard()?["text"] == "Shared clipboard 中文",
            "Host to guest clipboard mismatch"
        );
        fixture("clipboard", json!({"text":"Android copy 😀"}))?;
        wait(&|| {
            let v = guest.clipboard()?;
            Ok((v["text"] == "Android copy 😀").then_some(v))
        })?;
        ensure!(guest.set_clipboard(json!({"kind":"text","text":"stale","origin":"test","expected":before["version"]})).is_err(),"Stale clipboard version overwrote a new copy");
        guest.set_clipboard(json!({"kind":"files","paths":[source],"origin":"test-files"}))?;
        let file_clip = guest.clipboard()?;
        let copied = guest.materialize_clipboard(&file_clip)?;
        ensure!(
            file_clip["kind"] == "files"
                && copied.len() == 1
                && digest(&copied[0])? == digest(&source)?,
            "File clipboard bytes changed"
        );
        use base64::Engine;
        let png = tmp.path().join("fixture.png");
        fs::write(&png,base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jSugAAAAASUVORK5CYII=")?)?;
        for action in [
            "android.provider.action.PICK_IMAGES",
            "android.intent.action.PICK",
        ] {
            fixture(
                "choose",
                json!({"action":action,"mime":"image/png","maximum":150}),
            )?;
            let r: PickerRequest = serde_json::from_value(wait(&|| {
                Ok(guest
                    .file_events()?
                    .requests
                    .into_iter()
                    .find(|r| r.package == "dev.glidepod.filefixture")
                    .map(|r| serde_json::to_value(r).unwrap()))
            })?)?;
            if action == "android.provider.action.PICK_IMAGES" {
                ensure!(
                    r.maximum == 64,
                    "Platform photo maximum was not gracefully capped"
                );
            }
            guest.complete_picker(&r, std::slice::from_ref(&png), false)?;
            ensure!(
                result()?["items"][0]["sha256"] == digest(&png)?,
                "Image picker bytes changed"
            );
        }
        guest.set_clipboard(json!({"kind":"image","paths":[png],"origin":"test-image"}))?;
        let image_clip = guest.clipboard()?;
        let copied = guest.materialize_clipboard(&image_clip)?;
        ensure!(
            image_clip["kind"] == "image" && digest(&copied[0])? == digest(&png)?,
            "Image clipboard bytes changed"
        );
        window.edit_action("paste")?;
        wait(&|| {
            let v = fixture("result", json!({}))?;
            Ok((v["rich_hash"] == digest(&png)?).then_some(v))
        })?;
        window.edit_action("select_all")?;
        guest.set_clipboard(
            json!({"kind":"text","text":"System paste 中文","origin":"test-paste"}),
        )?;
        window.edit_action("paste")?;
        wait(&|| {
            let v = fixture("editor", json!({}))?;
            Ok((v["text"] == "System paste 中文").then_some(v))
        })?;
        window.edit_action("select_all")?;
        window.edit_action("copy")?;
        wait(&|| {
            let v = guest.clipboard()?;
            Ok((v["text"] == "System paste 中文").then_some(v))
        })?;
        ensure!(
            fixture("private_read", json!({}))? == private,
            "App private data changed"
        );
        fixture("share", json!({}))?;
        wait(&|| Ok((!guest.file_events()?.exports.is_empty()).then_some(json!(true))))?;
        let shared = exporter.cycle()?;
        ensure!(
            shared.len() == 1 && digest(&shared[0])? == private["sha256"],
            "Private attachment Share export mismatch"
        );
        let evidence = std::env::var_os("GLIDEPOD_TEST_EVIDENCE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("evidence/host-files-2026-11-07"));
        fs::create_dir_all(&evidence)?;
        atomic_json(
            &evidence.join("guest-contracts.json"),
            &json!({"passed":true,"epoch":guest.endpoints.epoch,"checks":["unprivileged transport rejection","OPEN_DOCUMENT bytes and persistent grant across app restart","independent imported snapshot","cancel","multi-select","CREATE_DOCUMENT deferred write","idempotent export","pending download exclusion","published MediaStore download","successful public DownloadManager export","private DownloadManager destination excluded","host edits preserve Android source","no recreation after host deletion","targeted Share delivery","bidirectional Android clipboard API","stale clipboard rejection","file and PNG clipboard materialization","system and legacy image picker","actual rich PNG IME insertion","normal editor paste and copy","private app sentinel","private attachment Share export"],"native_panels_verified":false}),
        )?;
        Ok(())
    }
}
