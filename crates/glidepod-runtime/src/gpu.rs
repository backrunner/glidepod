//! Explicitly registered external GPU libraries. No renderer is bundled or auto-enabled.
use crate::{ImageFile, Store};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
const COMMON_FILES: &[&str] = &[
    "source.properties",
    "lib64/libgfxstream_backend.dylib",
    "lib64/vulkan/libvulkan.dylib",
];
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VulkanDriver {
    #[default]
    MoltenVk,
    KosmicKrisp,
}
impl VulkanDriver {
    pub fn label(self) -> &'static str {
        match self {
            Self::MoltenVk => "MoltenVK",
            Self::KosmicKrisp => "KosmicKrisp",
        }
    }
    pub fn icd(self) -> &'static str {
        match self {
            Self::MoltenVk => "moltenvk",
            Self::KosmicKrisp => "kosmickrisp",
        }
    }
    fn library(self) -> &'static str {
        match self {
            Self::MoltenVk => "libMoltenVK.dylib",
            Self::KosmicKrisp => "libvulkan_kosmickrisp.dylib",
        }
    }
    fn manifest(self) -> &'static str {
        match self {
            Self::MoltenVk => "MoltenVK_icd.json",
            Self::KosmicKrisp => "libkosmickrisp_icd.json",
        }
    }
    fn files(self) -> Vec<String> {
        COMMON_FILES
            .iter()
            .map(|s| (*s).to_owned())
            .chain([
                format!("lib64/vulkan/{}", self.library()),
                format!("lib64/vulkan/{}", self.manifest()),
            ])
            .collect()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuRegistry {
    pub schema_version: u32,
    pub selected_driver: Option<VulkanDriver>,
    pub renderers: Vec<ExperimentalGpu>,
}
impl Default for GpuRegistry {
    fn default() -> Self {
        Self {
            schema_version: 2,
            selected_driver: None,
            renderers: Vec::new(),
        }
    }
}
impl GpuRegistry {
    pub fn selected(&self) -> Option<&ExperimentalGpu> {
        self.renderers
            .iter()
            .find(|r| Some(r.driver) == self.selected_driver)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 2 && self.renderers.len() <= 2,
            "Invalid GPU registry"
        );
        for (i, renderer) in self.renderers.iter().enumerate() {
            ensure!(
                !self.renderers[..i]
                    .iter()
                    .any(|r| r.driver == renderer.driver),
                "Duplicate GPU driver registration"
            );
        }
        ensure!(
            self.renderers.is_empty() && self.selected_driver.is_none()
                || self.selected().is_some(),
            "Selected GPU driver is not registered"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentalGpu {
    pub directory: PathBuf,
    /// Registrations made before driver selection used MoltenVK.
    #[serde(default)]
    pub driver: VulkanDriver,
    pub files: Vec<ImageFile>,
}
impl ExperimentalGpu {
    pub fn from_local(directory: &Path) -> Result<Self> {
        Self::from_local_with_driver(directory, VulkanDriver::MoltenVk)
    }
    pub fn from_local_with_driver(directory: &Path, driver: VulkanDriver) -> Result<Self> {
        let directory = directory.canonicalize()?;
        let mut files = Vec::new();
        for name in driver.files() {
            let path = directory.join(&name);
            ensure!(!path.is_symlink(), "GPU artifacts must not be symlinks");
            files.push(ImageFile {
                path: name,
                bytes: path.metadata()?.len(),
                sha256: crate::experimental::digest(&path)?,
            });
        }
        let gpu = Self {
            directory,
            driver,
            files,
        };
        gpu.verify_manifest()?;
        Ok(gpu)
    }
    pub fn verify(&self) -> Result<()> {
        ensure!(
            self.directory.is_absolute() && self.files.len() == self.driver.files().len(),
            "Invalid GPU registration"
        );
        for name in self.driver.files() {
            let expected = self
                .files
                .iter()
                .filter(|f| f.path == name)
                .collect::<Vec<_>>();
            ensure!(expected.len() == 1, "Incomplete GPU registration");
            let path = self.directory.join(&name);
            ensure!(
                !path.is_symlink()
                    && path.metadata()?.len() == expected[0].bytes
                    && crate::experimental::digest(&path)? == expected[0].sha256,
                "GPU artifact changed: {name}"
            );
        }
        self.verify_manifest()
    }
    fn verify_manifest(&self) -> Result<()> {
        let file = File::open(
            self.directory
                .join("lib64/vulkan")
                .join(self.driver.manifest()),
        )?;
        ensure!(
            file.metadata()?.len() <= 16384,
            "GPU driver manifest exceeds limit"
        );
        let manifest: serde_json::Value = serde_json::from_reader(file)?;
        let path = manifest["ICD"]["library_path"].as_str().unwrap_or_default();
        ensure!(
            path == self.driver.library() || path == format!("./{}", self.driver.library()),
            "GPU driver manifest must reference its registered library"
        );
        Ok(())
    }
}
impl Store {
    pub fn experimental_gpu(&self) -> Result<Option<ExperimentalGpu>> {
        Ok(self.gpu_registry()?.selected().cloned())
    }
    pub fn gpu_registry(&self) -> Result<GpuRegistry> {
        let path = self.root().join("experimental-gpu.json");
        if !path.exists() {
            return Ok(GpuRegistry::default());
        }
        let file = File::open(path)?;
        ensure!(
            file.metadata()?.len() <= 32768,
            "GPU registration exceeds limit"
        );
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Registry(GpuRegistry),
            Legacy(ExperimentalGpu),
        }
        let registry = match serde_json::from_reader(file)? {
            Stored::Registry(registry) => registry,
            Stored::Legacy(gpu) => GpuRegistry {
                selected_driver: Some(gpu.driver),
                renderers: vec![gpu],
                ..GpuRegistry::default()
            },
        };
        registry.validate()?;
        Ok(registry)
    }
    pub fn register_gpu(&self, directory: &Path) -> Result<ExperimentalGpu> {
        self.register_gpu_with_driver(directory, VulkanDriver::MoltenVk)
    }
    pub fn register_gpu_with_driver(
        &self,
        directory: &Path,
        driver: VulkanDriver,
    ) -> Result<ExperimentalGpu> {
        let _maintenance = self.maintenance_lock(false)?;
        let _guard = self.session_lock()?;
        let gpu = ExperimentalGpu::from_local_with_driver(directory, driver)?;
        gpu.verify()?;
        let mut registry = self.gpu_registry()?;
        registry.renderers.retain(|r| r.driver != driver);
        registry.renderers.push(gpu.clone());
        registry.selected_driver = Some(driver);
        crate::experimental::atomic_json(&self.root().join("experimental-gpu.json"), &registry)?;
        Ok(gpu)
    }
    pub fn select_gpu_driver(&self, driver: VulkanDriver) -> Result<ExperimentalGpu> {
        let _maintenance = self.maintenance_lock(false)?;
        let _guard = self.session_lock()?;
        let mut registry = self.gpu_registry()?;
        let gpu = registry
            .renderers
            .iter()
            .find(|r| r.driver == driver)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Register the {} renderer before selecting it",
                    driver.label()
                )
            })?
            .clone();
        gpu.verify()?;
        registry.selected_driver = Some(driver);
        crate::experimental::atomic_json(&self.root().join("experimental-gpu.json"), &registry)?;
        Ok(gpu)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn artifacts(dir: &Path, driver: VulkanDriver) {
        for name in driver.files() {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"reviewed").unwrap();
        }
        std::fs::write(
            dir.join("lib64/vulkan").join(driver.manifest()),
            serde_json::to_vec(&serde_json::json!({"ICD":{"library_path":driver.library()}}))
                .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn changed_renderer_is_rejected_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        artifacts(dir.path(), VulkanDriver::MoltenVk);
        let gpu = ExperimentalGpu::from_local(dir.path()).unwrap();
        gpu.verify().unwrap();
        std::fs::write(dir.path().join(COMMON_FILES[1]), b"modified").unwrap();
        assert!(gpu.verify().is_err());
    }
    #[test]
    fn registration_cannot_substitute_an_uninventoried_path() {
        let gpu = ExperimentalGpu {
            directory: PathBuf::from("/tmp"),
            driver: VulkanDriver::MoltenVk,
            files: vec![],
        };
        assert!(gpu.verify().is_err());
    }
    #[test]
    fn legacy_registration_retains_moltenvk() {
        let dir = tempfile::tempdir().unwrap();
        artifacts(dir.path(), VulkanDriver::MoltenVk);
        let gpu = ExperimentalGpu::from_local(dir.path()).unwrap();
        let mut json = serde_json::to_value(gpu).unwrap();
        json.as_object_mut().unwrap().remove("driver");
        let legacy: ExperimentalGpu = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.driver, VulkanDriver::MoltenVk);
        legacy.verify().unwrap();
    }
    #[test]
    fn selected_driver_is_pinned_and_manifest_cannot_redirect_it() {
        let dir = tempfile::tempdir().unwrap();
        let driver = VulkanDriver::KosmicKrisp;
        artifacts(dir.path(), driver);
        let gpu = ExperimentalGpu::from_local_with_driver(dir.path(), driver).unwrap();
        gpu.verify().unwrap();
        assert!(ExperimentalGpu::from_local(dir.path()).is_err());
        std::fs::write(
            dir.path().join("lib64/vulkan").join(driver.library()),
            b"modified",
        )
        .unwrap();
        assert!(gpu.verify().is_err());
        artifacts(dir.path(), driver);
        std::fs::write(
            dir.path().join("lib64/vulkan").join(driver.manifest()),
            br#"{"ICD":{"library_path":"/tmp/unregistered.dylib"}}"#,
        )
        .unwrap();
        assert!(ExperimentalGpu::from_local_with_driver(dir.path(), driver).is_err());
    }
    #[test]
    fn driver_switch_preserves_both_registrations_and_rejects_changed_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().join("store")).unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        artifacts(&a, VulkanDriver::MoltenVk);
        artifacts(&b, VulkanDriver::KosmicKrisp);
        let legacy = ExperimentalGpu::from_local(&a).unwrap();
        crate::experimental::atomic_json(&store.root().join("experimental-gpu.json"), &legacy)
            .unwrap();
        store
            .register_gpu_with_driver(&b, VulkanDriver::KosmicKrisp)
            .unwrap();
        assert_eq!(store.gpu_registry().unwrap().renderers.len(), 2);
        store.select_gpu_driver(VulkanDriver::MoltenVk).unwrap();
        let reopened = Store::new(store.root().to_owned()).unwrap();
        assert_eq!(
            reopened.experimental_gpu().unwrap().unwrap().driver,
            VulkanDriver::MoltenVk
        );
        std::fs::write(
            b.join("lib64/vulkan/libvulkan_kosmickrisp.dylib"),
            b"changed",
        )
        .unwrap();
        assert!(
            reopened
                .select_gpu_driver(VulkanDriver::KosmicKrisp)
                .is_err()
        );
        assert_eq!(
            reopened.gpu_registry().unwrap().selected_driver,
            Some(VulkanDriver::MoltenVk)
        );
    }
}
