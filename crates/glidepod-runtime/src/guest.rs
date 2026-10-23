//! Typed Android operations over an epoch-scoped private transport.
use crate::{
    Store,
    adb::{Adb, quote},
};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use fs2::FileExt;
use glidepod_core::VmState;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoints {
    pub epoch: u64,
    pub vsock: PathBuf,
    pub control: PathBuf,
    pub frames: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndroidApp {
    pub package_id: String,
    pub component: String,
    pub label: String,
    pub version: String,
    pub system: bool,
    pub game: bool,
    pub updated: u64,
    #[serde(default)]
    pub icon_png: String,
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct InstallReceipt {
    pub package_id: String,
    pub sha256: String,
    pub epoch: u64,
    pub android_result: String,
}

pub fn package_id(value: &str) -> Result<()> {
    ensure!(!value.is_empty() && value.len() <= 255 && value.contains('.') && value.split('.').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')), "Invalid Android package ID");
    Ok(())
}
pub fn resource(name: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let paths = [
        std::env::var_os("GLIDEPOD_ANDROID_RESOURCES").map(PathBuf::from),
        exe.parent()
            .and_then(Path::parent)
            .map(|p| p.join("Resources/android")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/android")),
    ];
    paths
        .into_iter()
        .flatten()
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .with_context(|| {
            format!("Missing Android helper {name}; run tools/build-android.py and package again")
        })
}
impl Store {
    pub fn guest(&self) -> Result<Guest> {
        let status = self.runtime_status()?;
        let VmState::Running { epoch } = status.vm else {
            anyhow::bail!("Start Android before using apps");
        };
        let endpoints = status
            .endpoints
            .context("Restart Android to enable the updated app bridge")?;
        ensure!(endpoints.epoch == epoch, "Expired Android transport");
        Ok(Guest {
            store: self.clone(),
            endpoints,
        })
    }
    pub fn cached_apps(&self) -> Result<Vec<AndroidApp>> {
        if self.load()?.active_profile != glidepod_core::ProfileId::Developer {
            return Ok(Vec::new());
        }
        let path = self.root().join("developer/apps.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(path)?;
        ensure!(
            file.metadata()?.len() <= 8 * 1024 * 1024,
            "App catalog exceeds limit"
        );
        Ok(serde_json::from_reader(file)?)
    }
}
#[derive(Clone)]
pub struct Guest {
    pub(crate) store: Store,
    pub endpoints: Endpoints,
}
impl Guest {
    pub(crate) fn provider(
        &self,
        authority: &str,
        method: &str,
        arg: Option<&str>,
    ) -> Result<String> {
        let result = self.shell(&format!(
            "content call --uri {} --method {} {}",
            quote(&format!("content://{authority}")),
            quote(method),
            arg.map(|a| format!("--arg {}", quote(a)))
                .unwrap_or_default()
        ))?;
        ensure!(
            result.len() <= 2 * 1024 * 1024,
            "Provider response exceeds limit"
        );
        result
            .trim()
            .strip_prefix("Result: Bundle[{json=")
            .and_then(|s| s.strip_suffix("}]"))
            .map(str::to_owned)
            .context("Android helper provider returned an invalid response")
    }
    pub(crate) fn bridge(&self, class: &str, args: &[&str]) -> Result<String> {
        self.verify_epoch()?;
        let path = resource("glidepod-bridge.jar")?;
        // Content-addressed paths avoid replacing a JAR while another helper is reading it.
        let hash = crate::experimental::digest(&path)?;
        let remote = format!("/data/local/tmp/glidepod/bridge-{hash}.jar");
        let marker = self.store.root().join("developer/bridge-session.txt");
        let value = format!("{}:{hash}", self.endpoints.epoch);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.store.root().join("bridge.lock"))?;
        lock.lock_exclusive()?;
        if fs::read_to_string(&marker).ok().as_deref() != Some(&value) {
            self.shell("mkdir -p /data/local/tmp/glidepod")?;
            Adb::push(&self.endpoints.vsock, &path, &remote)?;
            fs::write(marker, value)?;
        }
        drop(lock);
        self.shell(&format!(
            "CLASSPATH={} app_process / {} {}",
            quote(&remote),
            quote(class),
            args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
        ))
    }
    pub(crate) fn prepare_input(&self) -> Result<()> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.store.root().join("input.lock"))?;
        lock.lock_exclusive()?;
        let apk = resource("glidepod-input.apk")?;
        let bridge = resource("glidepod-bridge.jar")?;
        let hash = format!(
            "{}:{}",
            crate::experimental::digest(&apk)?,
            crate::experimental::digest(&bridge)?
        );
        let marker = self.store.root().join("developer/input-session.txt");
        let value = format!("{}:{hash}", self.endpoints.epoch);
        if fs::read_to_string(&marker).ok().as_deref() == Some(&value) {
            return Ok(());
        }
        self.shell("mkdir -p /data/local/tmp/glidepod")?;
        Adb::push(
            &self.endpoints.vsock,
            &bridge,
            "/data/local/tmp/glidepod/input-bridge.jar",
        )?;
        self.install(&apk, None)?;
        self.shell(
            "ime enable dev.glidepod.input/.MacInput && ime set dev.glidepod.input/.MacInput",
        )?;
        fs::write(marker, value)?;
        Ok(())
    }
    pub fn verify_epoch(&self) -> Result<()> {
        ensure!(
            matches!(self.store.runtime_status()?.vm, VmState::Running{epoch} if epoch == self.endpoints.epoch),
            "Android restarted or stopped; reopen the app"
        );
        Ok(())
    }
    pub(crate) fn shell(&self, command: &str) -> Result<String> {
        self.shell_timeout(command, Duration::from_secs(30))
    }
    pub(crate) fn shell_timeout(&self, command: &str, timeout: Duration) -> Result<String> {
        self.verify_epoch()?;
        Adb::shell(&self.endpoints.vsock, command, timeout)?.text()
    }
    pub fn catalog(&self) -> Result<Vec<AndroidApp>> {
        self.shell("mkdir -p /data/local/tmp/glidepod")?;
        Adb::push(
            &self.endpoints.vsock,
            &resource("glidepod-bridge.jar")?,
            "/data/local/tmp/glidepod/bridge.jar",
        )?;
        let json = self.shell(
            "CLASSPATH=/data/local/tmp/glidepod/bridge.jar app_process / dev.glidepod.bridge.Main",
        )?;
        let mut apps: Vec<AndroidApp> =
            serde_json::from_str(json.trim()).context("Read Android app catalog")?;
        ensure!(apps.len() <= 4096, "Too many installed apps");
        let icons = self.store.root().join("developer/icons");
        fs::create_dir_all(&icons)?;
        for app in &mut apps {
            package_id(&app.package_id)?;
            ensure!(
                app.component.starts_with(&format!("{}/", app.package_id)),
                "Invalid activity component"
            );
            if !app.icon_png.is_empty() {
                ensure!(app.icon_png.len() <= 128 * 1024, "App icon exceeds limit");
                let bytes = base64::engine::general_purpose::STANDARD.decode(&app.icon_png)?;
                let path = icons.join(format!("{}.png", app.package_id));
                fs::write(&path, bytes)?;
                app.icon_path = Some(path);
                app.icon_png.clear();
            }
        }
        apps.sort_by_key(|a| a.label.to_lowercase());
        let mut file = tempfile::NamedTempFile::new_in(icons.parent().unwrap())?;
        serde_json::to_writer(&mut file, &apps)?;
        file.as_file().sync_all()?;
        file.persist(self.store.root().join("developer/apps.json"))?;
        Ok(apps)
    }
    pub fn install(&self, source: &Path, reviewed_sha256: Option<&str>) -> Result<InstallReceipt> {
        self.verify_epoch()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.store.root().join("install.lock"))?;
        lock.try_lock_exclusive()
            .context("Another APK installation is in progress")?;
        // Inspect exactly the private snapshot sent to Package Manager, not the mutable source path.
        let mut staged = tempfile::Builder::new()
            .suffix(".apk")
            .tempfile_in(self.store.root())?;
        let mut input = File::open(source)?.take(2 * 1024 * 1024 * 1024 + 1);
        let count = std::io::copy(&mut input, &mut staged)?;
        ensure!(count <= 2 * 1024 * 1024 * 1024, "APK exceeds 2 GiB");
        staged.flush()?;
        let report = glidepod_installer::inspect(staged.path())?;
        ensure!(
            report.arm64_candidate,
            "This APK has no ARM64-compatible native libraries"
        );
        if let Some(expected) = reviewed_sha256 {
            ensure!(
                expected == report.sha256,
                "APK changed after review. Review it again before installing."
            );
        }
        package_id(&report.manifest.package_id)?;
        self.shell("mkdir -p /data/local/tmp/glidepod")?;
        let destination = format!("/data/local/tmp/glidepod/install-{}.apk", report.sha256);
        Adb::push(&self.endpoints.vsock, staged.path(), &destination)?;
        let result = self.shell_timeout(
            &format!("pm install -r {}", quote(&destination)),
            Duration::from_secs(180),
        );
        let _ = self.shell(&format!("rm -f {}", quote(&destination)));
        let result = result?;
        ensure!(
            result.lines().any(|s| s.trim() == "Success"),
            "Android rejected the APK: {result}"
        );
        Ok(InstallReceipt {
            package_id: report.manifest.package_id,
            sha256: report.sha256,
            epoch: self.endpoints.epoch,
            android_result: result.trim().into(),
        })
    }
    pub fn uninstall(&self, package: &str) -> Result<()> {
        package_id(package)?;
        let apps = self.catalog()?;
        let app = apps
            .iter()
            .find(|a| a.package_id == package)
            .context("App is not installed")?;
        ensure!(!app.system, "System apps cannot be removed here");
        let result = self.shell(&format!("pm uninstall {}", quote(package)))?;
        ensure!(
            result.trim() == "Success",
            "Android could not remove the app: {result}"
        );
        self.catalog()?;
        Ok(())
    }
    pub fn stop_app(&self, package: &str) -> Result<()> {
        package_id(package)?;
        self.shell(&format!("am force-stop {}", quote(package)))?;
        Ok(())
    }
    pub fn developer_shell(&self, command: &str, root: bool) -> Result<String> {
        ensure!(
            command.len() <= 32768 && !command.contains('\0'),
            "Invalid shell command"
        );
        if root {
            ensure!(
                self.store.load()?.developer.root_requested,
                "Enable Developer root commands in Android settings first"
            );
        }
        self.shell(&if root {
            format!("su 0 sh -c {}", quote(command))
        } else {
            command.into()
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_names_cannot_escape_paths_or_commands() {
        for s in ["", "../x", "dev.foo;id", "dev..foo", "dev/foo", "dev.foo\n"] {
            assert!(package_id(s).is_err());
        }
        assert!(package_id("dev.glidepod.fixture").is_ok());
    }
}
