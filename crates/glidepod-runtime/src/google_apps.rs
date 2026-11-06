//! Transactional optional GMS installation. The current instance is never modified
//! until a stopped, verified candidate is ready for an atomic directory promotion.
mod package;
use crate::{
    Store,
    adb::{Adb, quote},
    experimental::{RuntimeOwner, atomic_json, digest},
    guest::Guest,
};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use glidepod_core::{ProfileId, VmState};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub message: String,
    pub completed: u64,
    pub total: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Remove,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installation {
    pub schema_version: u32,
    pub package_sha256: String,
    pub image_identity: String,
    pub files: Vec<String>,
    pub state: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    schema_version: u32,
    phase: String,
    had_original: bool,
}
const WORK: &str = ".g";
const RECORD: &str = "google-apps.json";
const JOURNAL: &str = "google-apps-transaction.json";
fn emit(progress: &mut dyn FnMut(Progress), message: &str) {
    progress(Progress {
        message: message.into(),
        completed: 0,
        total: 0,
    });
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Google services operation cancelled"
    );
    Ok(())
}
fn disk_lock(instance: &Path) -> Result<Option<File>> {
    let path = instance.join("vm.lock");
    if !path.exists() {
        return Ok(None);
    }
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    file.try_lock_exclusive()
        .context("An Android engine still owns this disk; wait for shutdown before recovery")?;
    Ok(Some(file))
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn record<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic_json(path, value)?;
    sync_dir(path.parent().context("Missing parent")?)
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let f = File::open(path)?;
    ensure!(
        f.metadata()?.len() < 1024 * 1024,
        "Oversized Google services record"
    );
    Ok(serde_json::from_reader(f)?)
}
fn image_identity(store: &Store) -> Result<String> {
    use sha2::{Digest, Sha256};
    let runtime = store
        .experimental_runtime()?
        .context("Register an Android runtime first")?;
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        runtime.expected_fingerprint,
        runtime.files,
    ))?)))
}
impl Store {
    pub(crate) fn maintenance_lock(&self, exclusive: bool) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.root().join("maintenance.lock"))?;
        if exclusive {
            file.try_lock_exclusive()
        } else {
            FileExt::try_lock_shared(&file)
        }
        .context("Google services maintenance is in progress")?;
        Ok(file)
    }
    pub fn google_apps_installation(&self) -> Result<Option<Installation>> {
        let p = self.root().join("developer/instance").join(RECORD);
        if !p.exists() {
            return Ok(None);
        }
        let state: Installation = read(&p)?;
        ensure!(
            state.schema_version == 1,
            "Unsupported Google services installation record"
        );
        Ok(Some(state))
    }
    pub fn google_apps_recovery_pending(&self) -> bool {
        self.root().join(JOURNAL).exists()
    }
    pub(crate) fn check_google_apps_image(&self) -> Result<()> {
        if let Some(state) = self.google_apps_installation()? {
            ensure!(
                state.image_identity == image_identity(self)?,
                "This Android image differs from the installed Google services image. Restore the previous runtime before starting or removing Google services."
            );
        }
        Ok(())
    }
    /// Caller owns the source session lock. A journal is never inferred from stale process IDs.
    pub(crate) fn recover_google_apps_locked(&self) -> Result<()> {
        let path = self.root().join(JOURNAL);
        if !path.exists() {
            return Ok(());
        }
        let journal: Journal = read(&path)?;
        ensure!(
            journal.schema_version == 1,
            "Unknown Google services recovery format"
        );
        ensure!(
            ["preparing", "promoting", "committed"].contains(&journal.phase.as_str()),
            "Unknown Google services recovery phase"
        );
        let work = self.root().join(WORK);
        let candidate = Store::new(work.clone())?;
        let deadline = Instant::now() + Duration::from_secs(35);
        let _candidate_lock = loop {
            if let Ok(lock) = candidate.session_lock() {
                break lock;
            }
            ensure!(
                Instant::now() < deadline,
                "The Google services candidate is still shutting down. Retry shortly."
            );
            thread::sleep(Duration::from_millis(100));
        };
        let instance = self.root().join("developer/instance");
        let backup = self.root().join("developer/.g-backup");
        let _original_disk = disk_lock(&instance)?;
        let _candidate_disk = disk_lock(&work.join("developer/instance"))?;
        if journal.phase != "committed" {
            if backup.exists() {
                if instance.exists() {
                    fs::remove_dir_all(&instance)?;
                }
                fs::rename(&backup, &instance)?;
            } else if !journal.had_original && journal.phase == "promoting" && instance.exists() {
                fs::remove_dir_all(&instance)?;
            }
        } else if backup.exists() {
            fs::remove_dir_all(backup)?;
        }
        // Keep the lock file until its store is removed; no worker can start without the private owner pipe.
        if work.exists() {
            fs::remove_dir_all(work)?;
        }
        let cache = self.root().join("google-apps-cache/payload");
        if cache.exists() {
            fs::remove_dir_all(cache)?;
        }
        let partial = self.root().join("google-apps-cache/package.partial");
        if partial.exists() {
            fs::remove_file(partial)?;
        }
        if self.root().join("developer").exists() {
            sync_dir(&self.root().join("developer"))?;
        }
        fs::remove_file(path)?;
        sync_dir(self.root())?;
        Ok(())
    }
    pub fn recover_google_apps(&self) -> Result<()> {
        let _maintenance = self.maintenance_lock(true)?;
        let _session = self.session_lock()?;
        self.recover_google_apps_locked()
    }
    /// Automatically stops Android. Successful transactions leave it stopped, ready to start.
    pub fn manage_google_apps(
        &self,
        helper: &Path,
        action: Action,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<()> {
        let _maintenance = self.maintenance_lock(true)?;
        ensure!(
            self.load()?.active_profile == ProfileId::Developer,
            "Google services currently require the Android Developer profile"
        );
        emit(progress, "Stopping Android…");
        if self.runtime_status()?.active() {
            // The UI lifetime pipe may already have requested shutdown. A closed
            // control socket does not prevent waiting for the actual session lock.
            let _ = self.stop_vm();
        }
        let deadline = Instant::now() + Duration::from_secs(35);
        let _session = loop {
            if let Ok(lock) = self.session_lock() {
                break lock;
            }
            ensure!(
                Instant::now() < deadline,
                "Android has not stopped. Retry after shutdown."
            );
            thread::sleep(Duration::from_millis(100));
        };
        self.recover_google_apps_locked()?;
        self.check_google_apps_image()?;
        check_cancel(cancel)?;
        let installed = self.google_apps_installation()?;
        if action == Action::Install && installed.is_some() {
            emit(progress, "Google services are already installed.");
            return Ok(());
        }
        ensure!(
            action != Action::Remove || installed.is_some(),
            "No GlidePod-managed Google services installation to remove"
        );
        let instance = self.root().join("developer/instance");
        ensure!(
            !self.root().join(WORK).exists() && !self.root().join("developer/.g-backup").exists(),
            "An unknown maintenance directory exists; existing files were preserved"
        );
        let mut journal = Journal {
            schema_version: 1,
            phase: "preparing".into(),
            had_original: instance.exists(),
        };
        record(&self.root().join(JOURNAL), &journal)?;
        let result = (|| -> Result<()> {
            let payload = if action == Action::Install {
                Some(package::prepare(self, cancel, progress)?)
            } else {
                None
            };
            check_cancel(cancel)?;
            emit(progress, "Preparing a recoverable Android copy…");
            ensure!(
                fs2::available_space(self.root())? > 2 * 1024 * 1024 * 1024,
                "Free at least 2 GiB for Android changes"
            );
            let work = Store::new(self.root().join(WORK))?;
            let mut configuration = self.load()?;
            configuration.notifications_enabled = false;
            record(&work.root().join("configuration.json"), &configuration)?;
            for name in ["experimental-runtime.json", "experimental-gpu.json"] {
                if self.root().join(name).exists() {
                    fs::copy(self.root().join(name), work.root().join(name))?;
                }
            }
            if instance.exists() {
                let _disk = disk_lock(&instance)?;
                clone_instance(&instance, &work.root().join("developer/instance"), cancel)?;
            }
            let mut candidate = Candidate {
                store: work,
                owner: None,
            };
            let result = (|| -> Result<()> {
                emit(progress, "Checking Android compatibility…");
                let mut guest = candidate.start(helper, cancel)?;
                let before = guest.google_services()?;
                ensure!(
                    before.inventory.sdk == package::source().sdk
                        && before.inventory.abis.iter().any(|a| a == "arm64-v8a"),
                    "Google services require Android 17 ARM64"
                );
                ensure!(
                    candidate.store.runtime_status()?.effective_root,
                    "This Android image does not support Google services installation"
                );
                if action == Action::Install {
                    ensure!(
                        before.inventory.packages.iter().all(|p| !p.installed),
                        "An unmanaged Google services installation already exists"
                    );
                }
                if root_shell(&guest, "avbctl get-verity")?.contains("verity is enabled") {
                    root_shell(&guest, "avbctl disable-verity")?;
                    candidate.stop()?;
                    guest = candidate.start(helper, cancel)?;
                }
                let remount = root_shell(&guest, "remount")?;
                if remount.to_lowercase().contains("reboot your device") {
                    candidate.stop()?;
                    guest = candidate.start(helper, cancel)?;
                    root_shell(&guest, "remount")?;
                }
                root_shell(
                    &guest,
                    "set -e; touch /product/.glidepod-probe /system_ext/.glidepod-probe; rm /product/.glidepod-probe /system_ext/.glidepod-probe; mount -o remount,async /mnt/scratch",
                )?;
                if let Some(payload) = &payload {
                    install_payload(&guest, payload, cancel, progress)?;
                } else {
                    remove_payload(&guest, installed.as_ref().unwrap(), cancel, progress)?;
                }
                check_cancel(cancel)?;
                emit(progress, "Restarting and checking Google services…");
                candidate.stop()?;
                guest = candidate.start(helper, cancel)?;
                let after = guest.google_services()?;
                if action == Action::Install {
                    ensure!(
                        after.inventory.packages.iter().all(|p| p.installed
                            && p.enabled
                            && p.system
                            && !p.signer_sha256.is_empty()),
                        "Google services did not pass installation checks"
                    );
                    let window = guest.open_app("com.android.vending", 720, 1080)?;
                    ensure!(window.task_count()? > 0, "Play Store could not open");
                    drop(window);
                } else {
                    ensure!(
                        after.inventory.packages.iter().all(|p| !p.installed),
                        "Google components remain; the original Android instance will be restored"
                    );
                }
                Ok(())
            })();
            let stopped = candidate.stop();
            result?;
            stopped?;
            check_cancel(cancel)?;
            let staged = candidate.store.root().join("developer/instance");
            for entry in fs::read_dir(&staged)? {
                let p = entry?.path();
                if p.extension().and_then(|e| e.to_str()) == Some("img") {
                    File::open(p)?.sync_all()?;
                }
            }
            if let Some(payload) = payload {
                record(
                    &staged.join(RECORD),
                    &Installation {
                        schema_version: 1,
                        package_sha256: package::source().sha256,
                        image_identity: image_identity(self)?,
                        files: payload.files,
                        state: "ready_for_sign_in".into(),
                    },
                )?;
            } else if staged.join(RECORD).exists() {
                fs::remove_file(staged.join(RECORD))?;
                sync_dir(&staged)?;
            }
            // Cancellation stops here. The journal makes both renames recoverable after process/power loss.
            emit(progress, "Applying the verified Android instance…");
            journal.phase = "promoting".into();
            record(&self.root().join(JOURNAL), &journal)?;
            fs::create_dir_all(self.root().join("developer"))?;
            if journal.had_original {
                fs::rename(&instance, self.root().join("developer/.g-backup"))?;
                sync_dir(&self.root().join("developer"))?;
            }
            fs::rename(&staged, &instance)?;
            sync_dir(&self.root().join("developer"))?;
            sync_dir(staged.parent().unwrap())?;
            journal.phase = "committed".into();
            record(&self.root().join(JOURNAL), &journal)?;
            Ok(())
        })();
        emit(
            progress,
            if result.is_ok() {
                "Finishing…"
            } else {
                "Restoring Android…"
            },
        );
        self.recover_google_apps_locked()
            .context("Recovery is pending. Start Android or select Recover to finish safely.")?;
        result?;
        // The verified archive is only a retry cache. Successful operations retain no installer payloads.
        let cache = self.root().join("google-apps-cache");
        if cache.exists() {
            fs::remove_dir_all(cache)?;
        }
        emit(
            progress,
            if action == Action::Install {
                "Google services installed. Start Android to sign in."
            } else {
                "Google services removed."
            },
        );
        Ok(())
    }
}
fn clone_instance(source: &Path, target: &Path, cancel: &AtomicBool) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        check_cancel(cancel)?;
        let entry = entry?;
        let from = entry.path();
        if from.extension().and_then(|s| s.to_str()) != Some("img") && entry.file_name() != RECORD {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file(),
            "Android disk must be a regular file"
        );
        let to = target.join(entry.file_name());
        let a = std::ffi::CString::new(from.as_os_str().as_encoded_bytes())?;
        let b = std::ffi::CString::new(to.as_os_str().as_encoded_bytes())?;
        #[cfg(target_os = "macos")]
        let cloned = unsafe { libc::clonefile(a.as_ptr(), b.as_ptr(), 0) } == 0;
        #[cfg(not(target_os = "macos"))]
        let cloned = {
            let _ = (a, b);
            false
        };
        if !cloned {
            ensure!(
                fs2::available_space(target)? > from.metadata()?.len() + 2 * 1024 * 1024 * 1024,
                "Not enough space to copy the Android disk on this filesystem"
            );
            fs::copy(from, &to)?;
        }
        File::open(to)?.sync_all()?;
    }
    sync_dir(target)
}
struct Candidate {
    store: Store,
    owner: Option<RuntimeOwner>,
}
impl Candidate {
    fn start(&mut self, helper: &Path, cancel: &AtomicBool) -> Result<Guest> {
        self.owner = Some(self.store.start_experimental(helper)?);
        let deadline = Instant::now() + Duration::from_secs(140);
        loop {
            check_cancel(cancel)?;
            match self.store.runtime_status()?.vm {
                VmState::Running { .. } => return self.store.guest(),
                VmState::Failed { message } => bail!("Android candidate failed: {message}"),
                _ => {}
            }
            ensure!(
                Instant::now() < deadline,
                "Android candidate boot timed out"
            );
            thread::sleep(Duration::from_millis(200));
        }
    }
    fn stop(&mut self) -> Result<()> {
        if self.store.runtime_status()?.active() {
            let _ = self.store.stop_vm();
        }
        self.owner.take();
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            if let Ok(_lock) = self.store.session_lock() {
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "Candidate shutdown timed out");
            thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Drop for Candidate {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn root_shell(guest: &Guest, script: &str) -> Result<String> {
    guest
        .shell_timeout(
            &format!(
                "su 0 sh -c {}",
                quote(&format!("set -e\n{script}\necho GLIDEPOD_OK"))
            ),
            Duration::from_secs(180),
        )
        .and_then(|s| {
            ensure!(
                s.lines().last() == Some("GLIDEPOD_OK"),
                "Android operation failed: {s}"
            );
            Ok(s)
        })
}
fn install_payload(
    guest: &Guest,
    payload: &package::Payload,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<()> {
    emit(progress, "Installing Google services in the recovery copy…");
    ensure!(
        guest.storage_overview()?.available_bytes > payload.bytes + 256 * 1024 * 1024,
        "Not enough free Android storage"
    );
    let stats = root_shell(guest, "stat -f -c '%a %S' /mnt/scratch")?;
    let fields: Vec<_> = stats
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        fields.len() == 2
            && fields[0].checked_mul(fields[1]).unwrap_or(0) > payload.bytes + 256 * 1024 * 1024,
        "Not enough free Android system overlay storage"
    );
    root_shell(
        guest,
        "mkdir -p /data/local/tmp/glidepod/google-apps; chown 2000:2000 /data/local/tmp/glidepod/google-apps; chmod 700 /data/local/tmp/glidepod/google-apps",
    )?;
    let mut total = 0;
    for path in &payload.files {
        check_cancel(cancel)?;
        let local = payload.directory.join(path);
        let remote = "/data/local/tmp/glidepod/google-apps/payload";
        root_shell(guest, &format!("test ! -e {}", quote(&format!("/{path}"))))?;
        Adb::push(&guest.endpoints.vsock, &local, remote)?;
        let hash = guest.shell_timeout(&format!("sha256sum {remote}"), Duration::from_secs(120))?;
        ensure!(
            hash.split_whitespace().next() == Some(digest(&local)?.as_str()),
            "Android payload digest mismatch"
        );
        let dest = format!("/{path}");
        let parent = Path::new(&dest).parent().unwrap().to_str().unwrap();
        root_shell(
            guest,
            &format!(
                "mkdir -p {}; cp {remote} {}; chown 0:0 {}; chmod 644 {}; rm {remote}",
                quote(parent),
                quote(&dest),
                quote(&dest),
                quote(&dest)
            ),
        )?;
        total += local.metadata()?.len();
        progress(Progress {
            message: "Installing Google services…".into(),
            completed: total,
            total: payload.bytes,
        });
    }
    root_shell(
        guest,
        "restorecon -RF /product /system_ext; sync; rmdir /data/local/tmp/glidepod/google-apps",
    )?;
    Ok(())
}
fn remove_payload(
    guest: &Guest,
    installed: &Installation,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<()> {
    emit(
        progress,
        "Removing Google services and their Android account data…",
    );
    // Only our pinned manifest can authorize removal. A host record never supplies arbitrary guest paths.
    ensure!(
        installed.package_sha256 == package::source().sha256,
        "Unknown managed Google services package"
    );
    for package in [
        "com.android.vending",
        "com.google.android.gsf",
        "com.google.android.gms",
        "com.google.android.partnersetup",
        "com.google.android.feedback",
    ] {
        check_cancel(cancel)?;
        let clear = root_shell(
            guest,
            &format!("if pm path {package} | grep -q '^package:'; then pm clear {package}; fi"),
        )?;
        ensure!(
            clear.contains("Success") || clear.trim() == "GLIDEPOD_OK",
            "Google component data could not be cleared: {package}"
        );
        // Android may return a nonzero status even after removing a system update.
        // The post-reboot package inventory is the authoritative removal gate.
        root_shell(
            guest,
            &format!("pm uninstall-system-updates {package} || true"),
        )?;
    }
    let expected: Vec<String> =
        serde_json::from_str(include_str!("../../../engine/google-apps-files.json"))?;
    ensure!(
        installed.files == expected,
        "Managed Google services manifest changed"
    );
    for path in &installed.files {
        ensure!(
            (path.starts_with("product/") || path.starts_with("system_ext/"))
                && path.split('/').all(|p| !p.is_empty()
                    && p != "."
                    && p != ".."
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
            "Invalid managed Google services path"
        );
    }
    let script = installed
        .files
        .iter()
        .map(|p| format!("rm -f {}\n", quote(&format!("/{p}"))))
        .collect::<String>();
    root_shell(
        guest,
        &format!(
            "{script}\nrm -f /data/apex/active/com.google.android.gmssystem.prodvic@*.apex /data/apex/decompressed/com.google.android.gmssystem.prodvic@*.decompressed.apex\nsync"
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_promotions_restore_original_and_committed_promotions_keep_new() -> Result<()> {
        for (phase, moved, activated) in [
            ("preparing", false, false),
            ("promoting", false, false),
            ("promoting", true, false),
            ("promoting", true, true),
            ("committed", true, true),
        ] {
            let temp = tempfile::tempdir()?;
            let store = Store::new(temp.path().to_path_buf())?;
            let instance = store.root().join("developer/instance");
            let backup = store.root().join("developer/.g-backup");
            fs::create_dir_all(&instance)?;
            fs::write(instance.join("userdata.img"), b"original user data")?;
            if moved {
                fs::rename(&instance, &backup)?;
            }
            if activated {
                fs::create_dir_all(&instance)?;
                fs::write(instance.join("userdata.img"), b"verified candidate")?;
            }
            record(
                &store.root().join(JOURNAL),
                &Journal {
                    schema_version: 1,
                    phase: phase.into(),
                    had_original: true,
                },
            )?;
            store.recover_google_apps()?;
            assert_eq!(
                fs::read(instance.join("userdata.img"))?,
                if phase == "committed" {
                    b"verified candidate".as_slice()
                } else {
                    b"original user data".as_slice()
                }
            );
            assert!(!backup.exists() && !store.google_apps_recovery_pending());
            store.recover_google_apps()?;
        }
        Ok(())
    }
    #[test]
    fn interrupted_first_install_removes_only_uncommitted_candidate() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::new(temp.path().to_path_buf())?;
        let instance = store.root().join("developer/instance");
        fs::create_dir_all(&instance)?;
        fs::write(instance.join("userdata.img"), b"unverified")?;
        fs::write(store.root().join("unrelated"), b"keep")?;
        record(
            &store.root().join(JOURNAL),
            &Journal {
                schema_version: 1,
                phase: "promoting".into(),
                had_original: false,
            },
        )?;
        store.recover_google_apps()?;
        assert!(!instance.exists());
        assert_eq!(fs::read(store.root().join("unrelated"))?, b"keep");
        Ok(())
    }
    #[test]
    fn maintenance_blocks_configuration_and_other_installers() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::new(temp.path().to_path_buf())?;
        let guard = store.maintenance_lock(true)?;
        assert!(store.maintenance_lock(true).is_err());
        assert!(store.update(0, |c| c.dark_mode = true).is_err());
        drop(guard);
        store.update(0, |c| c.dark_mode = true)?;
        Ok(())
    }
    #[test]
    fn cancellation_does_not_modify_source_disks() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::new(temp.path().to_path_buf())?;
        store.update(0, |c| c.active_profile = ProfileId::Developer)?;
        let dir = store.root().join("developer/instance");
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("userdata.img"), b"precious")?;
        let cancel = AtomicBool::new(true);
        assert!(
            store
                .manage_google_apps(Path::new("unused"), Action::Install, &cancel, &mut |_| {})
                .is_err()
        );
        assert_eq!(fs::read(dir.join("userdata.img"))?, b"precious");
        assert!(!store.google_apps_recovery_pending());
        Ok(())
    }
    #[test]
    fn failed_startup_recovery_is_reported_without_removing_unknown_state() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::new(temp.path().to_path_buf())?;
        record(
            &store.root().join(JOURNAL),
            &Journal {
                schema_version: 99,
                phase: "preparing".into(),
                had_original: false,
            },
        )?;
        assert!(crate::experimental::run_worker(&store).is_err());
        let status = store.runtime_status()?;
        assert!(
            matches!(status.vm, VmState::Failed { message } if message.contains("Unknown Google services recovery format"))
        );
        assert!(store.google_apps_recovery_pending());
        Ok(())
    }
    #[test]
    fn recovery_discards_partial_downloads_but_retains_the_retry_archive() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::new(temp.path().to_path_buf())?;
        let cache = store.root().join("google-apps-cache");
        fs::create_dir_all(cache.join("payload"))?;
        fs::write(cache.join("package.partial"), b"incomplete")?;
        fs::write(
            cache.join("package.zip"),
            b"retry archive checked again before use",
        )?;
        record(
            &store.root().join(JOURNAL),
            &Journal {
                schema_version: 1,
                phase: "preparing".into(),
                had_original: false,
            },
        )?;
        store.recover_google_apps()?;
        assert!(!cache.join("package.partial").exists());
        assert!(!cache.join("payload").exists());
        assert_eq!(
            fs::read(cache.join("package.zip"))?,
            b"retry archive checked again before use"
        );
        Ok(())
    }
}
