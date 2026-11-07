//! Install the original picker in a disposable copy, then promote only a verified,
//! stopped instance. The normal app data never participates in filesystem sharing.
use crate::{
    Store,
    adb::{Adb, quote},
    experimental::digest,
    google_apps::{
        Candidate, clone_instance, disk_lock, image_identity, record, root_shell, sync_dir,
    },
    guest::resource,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const JOURNAL: &str = "host-files-transaction.json";
const WORK: &str = ".f";
const RECORD: &str = "host-files.json";
#[derive(Serialize, Deserialize)]
struct Installation {
    version: u32,
    apk: String,
    image: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_promotion_restores_original_app_data() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let store = Store::new(tmp.path().join("store"))?;
        fs::create_dir_all(store.root().join("developer/.f-backup"))?;
        fs::write(
            store.root().join("developer/.f-backup/userdata.img"),
            "original private data",
        )?;
        fs::create_dir_all(store.root().join("developer/instance"))?;
        fs::write(
            store.root().join("developer/instance/userdata.img"),
            "uncommitted candidate",
        )?;
        Store::new(store.root().join(WORK))?;
        record(
            &store.root().join(JOURNAL),
            &Journal {
                version: 1,
                phase: "promoting".into(),
                had_original: true,
            },
        )?;
        store.recover_host_files()?;
        assert_eq!(
            fs::read_to_string(store.root().join("developer/instance/userdata.img"))?,
            "original private data"
        );
        assert!(!store.root().join(JOURNAL).exists());
        store.recover_host_files()?;
        Ok(())
    }
    #[test]
    fn committed_promotion_is_kept_and_unknown_state_is_preserved() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let store = Store::new(tmp.path().join("store"))?;
        fs::create_dir_all(store.root().join("developer/.f-backup"))?;
        fs::create_dir_all(store.root().join("developer/instance"))?;
        fs::write(
            store.root().join("developer/instance/userdata.img"),
            "verified candidate",
        )?;
        Store::new(store.root().join(WORK))?;
        record(
            &store.root().join(JOURNAL),
            &Journal {
                version: 1,
                phase: "committed".into(),
                had_original: true,
            },
        )?;
        store.recover_host_files()?;
        assert_eq!(
            fs::read_to_string(store.root().join("developer/instance/userdata.img"))?,
            "verified candidate"
        );
        record(
            &store.root().join(JOURNAL),
            &Journal {
                version: 2,
                phase: "unknown".into(),
                had_original: true,
            },
        )?;
        assert!(store.recover_host_files().is_err());
        assert!(store.root().join(JOURNAL).exists());
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    phase: String,
    had_original: bool,
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let f = File::open(path)?;
    ensure!(
        f.metadata()?.len() < 8192,
        "Invalid host integration record"
    );
    Ok(serde_json::from_reader(f.take(8192))?)
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Acquire), "Android startup cancelled");
    Ok(())
}
impl Store {
    // The caller owns this store's session lock and an exclusive maintenance lock.
    fn recover_host_files(&self) -> Result<()> {
        let journal_path = self.root().join(JOURNAL);
        if !journal_path.exists() {
            return Ok(());
        }
        let j: Journal = read(&journal_path)?;
        ensure!(
            j.version == 1 && ["preparing", "promoting", "committed"].contains(&j.phase.as_str()),
            "Unknown host integration recovery format"
        );
        let work = Store::new(self.root().join(WORK))?;
        let deadline = Instant::now() + Duration::from_secs(35);
        let _worker = loop {
            if let Ok(lock) = work.session_lock() {
                break lock;
            }
            ensure!(
                Instant::now() < deadline,
                "The integration candidate is still shutting down; retry shortly"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        let instance = self.root().join("developer/instance");
        let backup = self.root().join("developer/.f-backup");
        let _original = disk_lock(&instance)?;
        let _candidate = disk_lock(&work.root().join("developer/instance"))?;
        let _backup = disk_lock(&backup)?;
        if j.phase != "committed" {
            if backup.exists() {
                if instance.exists() {
                    fs::remove_dir_all(&instance)?;
                }
                fs::rename(&backup, &instance)?;
            } else if !j.had_original && j.phase == "promoting" && instance.exists() {
                fs::remove_dir_all(&instance)?;
            }
        } else if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        fs::remove_dir_all(work.root())?;
        if self.root().join("developer").exists() {
            sync_dir(&self.root().join("developer"))?;
        }
        fs::remove_file(journal_path)?;
        sync_dir(self.root())
    }
    pub(crate) fn prepare_host_files(&self, helper: &Path, cancel: &AtomicBool) -> Result<()> {
        let _maintenance = self.maintenance_lock(true)?;
        self.recover_host_files()?;
        cancelled(cancel)?;
        let apk = resource("glidepod-files.apk")?;
        let expected = Installation {
            version: 1,
            apk: digest(&apk)?,
            image: image_identity(self)?,
        };
        let instance = self.root().join("developer/instance");
        if instance.join(RECORD).exists() {
            let current: Installation = read(&instance.join(RECORD))?;
            ensure!(
                current.version == 1 && current.image == expected.image,
                "Host integration belongs to a different Android image; restore the registered runtime"
            );
            if current.apk == expected.apk {
                return Ok(());
            }
        }
        ensure!(
            !self.root().join(WORK).exists() && !self.root().join("developer/.f-backup").exists(),
            "Unrecognized integration work directory; preserved for inspection"
        );
        let mut j = Journal {
            version: 1,
            phase: "preparing".into(),
            had_original: instance.exists(),
        };
        record(&self.root().join(JOURNAL), &j)?;
        let result = (|| -> Result<()> {
            ensure!(
                fs2::available_space(self.root())? > 1024 * 1024 * 1024,
                "Free at least 1 GiB to prepare native file integration"
            );
            let work = Store::new(self.root().join(WORK))?;
            record(&work.root().join("configuration.json"), &self.load()?)?;
            for file in ["experimental-runtime.json", "experimental-gpu.json"] {
                if self.root().join(file).exists() {
                    fs::copy(self.root().join(file), work.root().join(file))?;
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
            let mut guest = candidate.start(helper, cancel)?;
            ensure!(
                candidate.store.runtime_status()?.effective_root,
                "This image needs a preinstalled GlidePod file bridge; the Developer installer requires guest root"
            );
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
            cancelled(cancel)?;
            guest.shell("mkdir -p /data/local/tmp/glidepod")?;
            Adb::push(
                &guest.endpoints.vsock,
                &apk,
                "/data/local/tmp/glidepod/files-system.apk",
            )?;
            let received = guest.shell_timeout(
                "sha256sum /data/local/tmp/glidepod/files-system.apk",
                Duration::from_secs(60),
            )?;
            ensure!(
                received.split_whitespace().next() == Some(expected.apk.as_str()),
                "System helper transfer digest mismatch"
            );
            root_shell(
                &guest,
                "mkdir -p /product/priv-app/GlidePodFiles; cp /data/local/tmp/glidepod/files-system.apk /product/priv-app/GlidePodFiles/GlidePodFiles.apk; chown 0:0 /product/priv-app/GlidePodFiles/GlidePodFiles.apk; chmod 644 /product/priv-app/GlidePodFiles/GlidePodFiles.apk; restorecon -RF /product/priv-app/GlidePodFiles; rm /data/local/tmp/glidepod/files-system.apk; sync",
            )?;
            candidate.stop()?;
            guest = candidate.start(helper, cancel)?;
            // A development reinstall can leave an updated system APK in /data.
            // Replace that overlay with the verified package without clearing data.
            let install = guest.shell_timeout(
                "pm install -r /product/priv-app/GlidePodFiles/GlidePodFiles.apk",
                Duration::from_secs(90),
            )?;
            ensure!(
                install.trim() == "Success",
                "Cannot refresh the system file bridge: {install}"
            );
            let installed_path = guest.shell("pm path dev.glidepod.files")?;
            let installed_path = installed_path
                .trim()
                .strip_prefix("package:")
                .context("Missing installed file bridge")?;
            ensure!(
                !installed_path.contains('\n'),
                "Unexpected split file bridge package"
            );
            let loaded = guest.shell(&format!("sha256sum {}", quote(installed_path)))?;
            ensure!(
                loaded.split_whitespace().next() == Some(expected.apk.as_str()),
                "Android loaded a different file bridge build"
            );
            let installed = guest.provider("dev.glidepod.files.bridge", "poll", None)?;
            ensure!(
                serde_json::from_str::<serde_json::Value>(&installed)?["system"] == true,
                "Android did not load the system file bridge"
            );
            for action in [
                "android.intent.action.OPEN_DOCUMENT",
                "android.intent.action.GET_CONTENT",
                "android.intent.action.CREATE_DOCUMENT",
                "android.provider.action.PICK_IMAGES",
            ] {
                let category = if action == "android.provider.action.PICK_IMAGES" {
                    ""
                } else {
                    "-c android.intent.category.OPENABLE"
                };
                let resolved = guest.shell(&format!(
                    "cmd package resolve-activity --brief -a {} {category} -t image/png",
                    quote(action)
                ))?;
                ensure!(
                    resolved.contains("dev.glidepod.files/.Picker")
                        || resolved.contains("dev.glidepod.files/dev.glidepod.files.Picker"),
                    "Android did not route {action} to the native file bridge: {resolved}"
                );
            }
            candidate.stop()?;
            cancelled(cancel)?;
            let staged = candidate.store.root().join("developer/instance");
            record(&staged.join(RECORD), &expected)?;
            for f in fs::read_dir(&staged)? {
                let p = f?.path();
                if p.extension().is_some_and(|e| e == "img") {
                    File::open(p)?.sync_all()?;
                }
            }
            j.phase = "promoting".into();
            record(&self.root().join(JOURNAL), &j)?;
            fs::create_dir_all(self.root().join("developer"))?;
            if j.had_original {
                fs::rename(&instance, self.root().join("developer/.f-backup"))?;
                sync_dir(&self.root().join("developer"))?;
            }
            fs::rename(&staged, &instance)?;
            sync_dir(&self.root().join("developer"))?;
            sync_dir(staged.parent().unwrap())?;
            j.phase = "committed".into();
            record(&self.root().join(JOURNAL), &j)?;
            Ok(())
        })();
        self.recover_host_files()
            .context("Host integration recovery is pending; restart Android to retry")?;
        result
    }
}
