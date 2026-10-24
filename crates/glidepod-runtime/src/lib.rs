//! Persistent configuration, image preflight and an opt-in experimental VM supervisor.
mod adb;
pub mod benchmark;
pub mod display;
pub mod experimental;
pub mod gpu;
pub mod guest;
pub mod input;
pub mod management;
pub mod notifications;
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use glidepod_core::{ProfileId, ProfileSettings, VmState};
use glidepod_protocol::Capabilities;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub schema_version: u32,
    pub revision: u64,
    pub active_profile: ProfileId,
    pub personal: ProfileSettings,
    pub developer: ProfileSettings,
    pub dark_mode: bool,
    #[serde(default)]
    pub audio_enabled: bool,
    #[serde(default)]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub recent_personal: Vec<String>,
    #[serde(default)]
    pub recent_developer: Vec<String>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            active_profile: ProfileId::Personal,
            personal: ProfileSettings::default(),
            developer: ProfileSettings::default(),
            dark_mode: false,
            audio_enabled: true,
            notifications_enabled: false,
            recent_personal: Vec::new(),
            recent_developer: Vec::new(),
        }
    }
}
impl Configuration {
    pub fn recent_apps(&self) -> &[String] {
        match self.active_profile {
            ProfileId::Personal => &self.recent_personal,
            ProfileId::Developer => &self.recent_developer,
        }
    }
    pub fn record_opened_app(&mut self, package: &str) {
        let recent = match self.active_profile {
            ProfileId::Personal => &mut self.recent_personal,
            ProfileId::Developer => &mut self.recent_developer,
        };
        recent.retain(|p| p != package);
        recent.insert(0, package.into());
        recent.truncate(20);
    }
    pub fn settings(&self) -> &ProfileSettings {
        match self.active_profile {
            ProfileId::Personal => &self.personal,
            ProfileId::Developer => &self.developer,
        }
    }
    pub fn settings_mut(&mut self) -> &mut ProfileSettings {
        match self.active_profile {
            ProfileId::Personal => &mut self.personal,
            ProfileId::Developer => &mut self.developer,
        }
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported configuration schema; the original file has been preserved."
        );
        self.personal.validate(ProfileId::Personal)?;
        self.developer.validate(ProfileId::Developer)?;
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn default_location() -> Result<Self> {
        let root = if let Some(path) = std::env::var_os("GLIDEPOD_DATA_DIR") {
            PathBuf::from(path)
        } else {
            PathBuf::from(
                std::env::var_os("HOME").context("HOME is unavailable; set GLIDEPOD_DATA_DIR")?,
            )
            .join("Library/Application Support/GlidePod")
        };
        Self::new(root)
    }
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn load(&self) -> Result<Configuration> {
        let path = self.root.join("configuration.json");
        if !path.exists() {
            return Ok(Configuration::default());
        }
        let file = File::open(&path)?;
        ensure!(
            file.metadata()?.len() <= 65536,
            "Configuration exceeds size limit; original file preserved."
        );
        let config: Configuration = serde_json::from_reader(file)
            .context("Invalid configuration; original file preserved")?;
        config.validate()?;
        Ok(config)
    }
    pub fn update(
        &self,
        expected_revision: u64,
        change: impl FnOnce(&mut Configuration),
    ) -> Result<Configuration> {
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let lock = options.open(self.root.join("configuration.lock"))?;
        lock.lock_exclusive()?;
        let mut config = self.load()?;
        ensure!(
            config.revision == expected_revision,
            "Settings changed in another client. Refresh and retry."
        );
        let old_profile = config.active_profile;
        change(&mut config);
        // A live userdebug guest must not be relabeled as the Personal profile.
        let _session_guard = if config.active_profile != old_profile {
            Some(
                self.session_lock()
                    .context("Stop Android before switching profiles")?,
            )
        } else {
            None
        };
        config.validate()?;
        config.revision = config
            .revision
            .checked_add(1)
            .context("Configuration revision overflow")?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        serde_json::to_writer_pretty(&mut temp, &config)?;
        temp.write_all(b"\n")?;
        temp.as_file().sync_all()?;
        temp.persist(self.root.join("configuration.json"))?;
        File::open(&self.root)?.sync_all()?;
        Ok(config)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let runtime = self.runtime_status()?;
        let apps = self.cached_apps()?;
        let bridge = matches!(runtime.vm, VmState::Running { .. }) && runtime.endpoints.is_some();
        Ok(Snapshot {
            configuration: self.load()?,
            vm: runtime.vm.clone(),
            capabilities: Capabilities {
                guest_root: runtime.effective_root,
                apk_install: bridge,
                task_displays: bridge,
                live_widgets: bridge && apps.iter().any(|a|a.package_id == "dev.glidepod.widgets"),
                gpu_rendering: bridge && runtime.effective_renderer == Some(glidepod_core::Renderer::Metal),
                ..Capabilities::default()
            },
            installed_apps: apps.len(),
            backend_ready: false,
            backend_message:
                "Experimental backend. Production distribution and game compatibility are not qualified."
                    .into(),
            runtime,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub configuration: Configuration,
    pub vm: VmState,
    pub capabilities: Capabilities,
    pub installed_apps: usize,
    pub backend_ready: bool,
    pub backend_message: String,
    pub runtime: experimental::RuntimeStatus,
}
pub fn start_vm() -> Result<()> {
    bail!(
        "No qualified Cuttlefish backend is installed. Run the G0 image/renderer qualification before starting Android."
    )
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageManifest {
    pub schema_version: u32,
    pub image_id: String,
    pub aosp_revision: String,
    pub build_fingerprint: String,
    pub architecture: String,
    pub guest_gfxstream_revision: String,
    pub host_gfxstream_revision: String,
    pub files: Vec<ImageFile>,
}
#[derive(Debug, Serialize)]
pub struct VerifiedImage {
    pub image_id: String,
    pub files_verified: usize,
    pub bytes_verified: u64,
    pub provenance_declared: bool,
    pub signature_verified: bool,
    pub boot_qualified: bool,
}
/// Checks bytes against a local manifest. A self-authored hash is not a publisher signature or a boot qualification.
pub fn verify_image(manifest_path: &Path) -> Result<VerifiedImage> {
    let file = File::open(manifest_path)?;
    ensure!(
        file.metadata()?.len() <= 1024 * 1024,
        "Image manifest exceeds 1 MiB"
    );
    let manifest: ImageManifest = serde_json::from_reader(file)?;
    ensure!(
        manifest.schema_version == 1 && manifest.architecture == "arm64",
        "Image requires schema 1 and arm64 architecture."
    );
    ensure!(
        !manifest.image_id.trim().is_empty()
            && !manifest.aosp_revision.trim().is_empty()
            && !manifest.build_fingerprint.trim().is_empty()
            && !manifest.guest_gfxstream_revision.trim().is_empty()
            && !manifest.host_gfxstream_revision.trim().is_empty(),
        "Image provenance is incomplete."
    );
    ensure!(
        !manifest.files.is_empty() && manifest.files.len() <= 64,
        "Image manifest must contain 1–64 files."
    );
    let base = manifest_path
        .canonicalize()?
        .parent()
        .context("Manifest has no parent")?
        .to_path_buf();
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for entry in &manifest.files {
        ensure!(names.insert(&entry.path), "Duplicate image path");
        ensure!(
            !entry.path.is_empty()
                && Path::new(&entry.path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "Image path must be relative without traversal."
        );
        ensure!(
            entry.sha256.len() == 64 && entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Expected a SHA-256 digest."
        );
        let path = base.join(&entry.path).canonicalize()?;
        ensure!(
            path.starts_with(&base),
            "Image symlink escapes its directory."
        );
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.len() == entry.bytes,
            "Image size mismatch: {}",
            entry.path
        );
        let mut hash = Sha256::new();
        let mut buf = [0u8; 65536];
        let mut bytes = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes = bytes.checked_add(n as u64).context("Image size overflow")?;
            ensure!(bytes <= entry.bytes, "Image changed while checking");
            hash.update(&buf[..n]);
        }
        ensure!(
            bytes == entry.bytes
                && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&entry.sha256),
            "Image digest mismatch: {}",
            entry.path
        );
        total = total.checked_add(bytes).context("Image size overflow")?;
    }
    Ok(VerifiedImage {
        image_id: manifest.image_id,
        files_verified: manifest.files.len(),
        bytes_verified: total,
        provenance_declared: true,
        signature_verified: false,
        boot_qualified: false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn image_fixture(dir: &Path) -> PathBuf {
        let bytes = b"Synthetic image fixture; not bootable";
        fs::write(dir.join("system.img"), bytes).unwrap();
        let manifest = ImageManifest {
            schema_version: 1,
            image_id: "test-image".into(),
            aosp_revision: "test-revision".into(),
            build_fingerprint: "test-fingerprint".into(),
            architecture: "arm64".into(),
            guest_gfxstream_revision: "test-guest".into(),
            host_gfxstream_revision: "test-host".into(),
            files: vec![ImageFile {
                path: "system.img".into(),
                bytes: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            }],
        };
        let path = dir.join("manifest.json");
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        path
    }
    #[test]
    fn verified_bytes_do_not_claim_signature_or_boot() {
        let dir = tempfile::tempdir().unwrap();
        let path = image_fixture(dir.path());
        let report = verify_image(&path).unwrap();
        assert_eq!(report.files_verified, 1);
        assert!(!report.signature_verified && !report.boot_qualified);
        let image = dir.path().join("system.img");
        let mut bytes = fs::read(&image).unwrap();
        bytes[0] ^= 1;
        fs::write(image, bytes).unwrap();
        assert!(
            verify_image(&path)
                .unwrap_err()
                .to_string()
                .contains("digest mismatch")
        );
    }
    #[test]
    fn image_paths_cannot_escape_manifest_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = image_fixture(dir.path());
        let mut manifest: ImageManifest =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest.files[0].path = "../system.img".into();
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            verify_image(&path)
                .unwrap_err()
                .to_string()
                .contains("traversal")
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_to_external_image_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = image_fixture(dir.path());
        fs::rename(
            dir.path().join("system.img"),
            outside.path().join("system.img"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("system.img"),
            dir.path().join("system.img"),
        )
        .unwrap();
        assert!(
            verify_image(&path)
                .unwrap_err()
                .to_string()
                .contains("symlink escapes")
        );
    }
    #[test]
    fn recent_apps_persist_in_open_order_without_crossing_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf()).unwrap();
        store
            .update(0, |c| {
                c.record_opened_app("dev.example.first");
                c.record_opened_app("dev.example.second");
                c.record_opened_app("dev.example.first");
            })
            .unwrap();
        let config = store.load().unwrap();
        assert_eq!(
            config.recent_apps(),
            &["dev.example.first", "dev.example.second"]
        );
        store
            .update(config.revision, |c| {
                c.active_profile = ProfileId::Developer;
                assert!(c.recent_apps().is_empty());
                for i in 0..25 {
                    c.record_opened_app(&format!("dev.example.app{i}"));
                }
            })
            .unwrap();
        let config = store.load().unwrap();
        assert_eq!(config.recent_apps().len(), 20);
        assert_eq!(config.recent_apps()[0], "dev.example.app24");
        assert_eq!(
            config.recent_personal,
            ["dev.example.first", "dev.example.second"]
        );
    }
    #[test]
    fn resource_preferences_are_isolated_by_profile() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf()).unwrap();
        let personal_memory = store.load().unwrap().personal.memory_mib;
        store
            .update(0, |c| {
                c.active_profile = ProfileId::Developer;
                c.settings_mut().memory_mib = 8192;
            })
            .unwrap();
        let config = store.load().unwrap();
        assert_eq!(config.personal.memory_mib, personal_memory);
        assert_eq!(config.developer.memory_mib, 8192);
    }
    #[test]
    fn stale_writer_cannot_erase_other_clients_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf()).unwrap();
        store.update(0, |c| c.dark_mode = true).unwrap();
        assert!(
            store
                .update(0, |c| c.active_profile = ProfileId::Developer)
                .is_err()
        );
        assert!(store.load().unwrap().dark_mode);
    }
    #[test]
    fn personal_root_does_not_reach_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf()).unwrap();
        assert!(
            store
                .update(0, |c| c.personal.root_requested = true)
                .is_err()
        );
        assert!(!store.load().unwrap().personal.root_requested);
    }
    #[test]
    fn corrupted_configuration_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf()).unwrap();
        let path = dir.path().join("configuration.json");
        fs::write(&path, b"broken").unwrap();
        assert!(store.load().is_err());
        assert!(store.update(0, |_| {}).is_err());
        assert_eq!(fs::read(path).unwrap(), b"broken");
    }
    #[test]
    fn missing_backend_never_reports_running() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = Store::new(dir.path().to_path_buf())
            .unwrap()
            .snapshot()
            .unwrap();
        assert!(!snapshot.backend_ready);
        assert!(!snapshot.capabilities.apk_install);
        assert!(start_vm().is_err());
    }
}
