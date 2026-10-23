//! Explicitly registered external GPU libraries. No renderer is bundled or auto-enabled.
use crate::{ImageFile, Store};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
const FILES: &[&str] = &[
    "source.properties",
    "lib64/libgfxstream_backend.dylib",
    "lib64/vulkan/libMoltenVK.dylib",
    "lib64/vulkan/libvulkan.dylib",
    "lib64/vulkan/MoltenVK_icd.json",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentalGpu {
    pub directory: PathBuf,
    pub files: Vec<ImageFile>,
}
impl ExperimentalGpu {
    pub fn from_local(directory: &Path) -> Result<Self> {
        let directory = directory.canonicalize()?;
        let mut files = Vec::new();
        for name in FILES {
            let path = directory.join(name);
            ensure!(!path.is_symlink(), "GPU artifacts must not be symlinks");
            files.push(ImageFile {
                path: name.to_string(),
                bytes: path.metadata()?.len(),
                sha256: crate::experimental::digest(&path)?,
            });
        }
        Ok(Self { directory, files })
    }
    pub fn verify(&self) -> Result<()> {
        ensure!(
            self.directory.is_absolute() && self.files.len() == FILES.len(),
            "Invalid GPU registration"
        );
        for name in FILES {
            let expected = self
                .files
                .iter()
                .filter(|f| f.path == *name)
                .collect::<Vec<_>>();
            ensure!(expected.len() == 1, "Incomplete GPU registration");
            let path = self.directory.join(name);
            ensure!(
                !path.is_symlink()
                    && path.metadata()?.len() == expected[0].bytes
                    && crate::experimental::digest(&path)? == expected[0].sha256,
                "GPU artifact changed: {name}"
            );
        }
        Ok(())
    }
}
impl Store {
    pub fn experimental_gpu(&self) -> Result<Option<ExperimentalGpu>> {
        let path = self.root().join("experimental-gpu.json");
        if !path.exists() {
            return Ok(None);
        }
        let file = File::open(path)?;
        ensure!(
            file.metadata()?.len() <= 16384,
            "GPU registration exceeds limit"
        );
        Ok(Some(serde_json::from_reader(file)?))
    }
    pub fn register_gpu(&self, directory: &Path) -> Result<ExperimentalGpu> {
        let _guard = self.session_lock()?;
        let gpu = ExperimentalGpu::from_local(directory)?;
        gpu.verify()?;
        crate::experimental::atomic_json(&self.root().join("experimental-gpu.json"), &gpu)?;
        Ok(gpu)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_renderer_is_rejected_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        for name in FILES {
            let p = dir.path().join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"reviewed").unwrap();
        }
        let gpu = ExperimentalGpu::from_local(dir.path()).unwrap();
        gpu.verify().unwrap();
        std::fs::write(dir.path().join(FILES[1]), b"modified").unwrap();
        assert!(gpu.verify().is_err());
    }
    #[test]
    fn registration_cannot_substitute_an_uninventoried_path() {
        let gpu = ExperimentalGpu {
            directory: PathBuf::from("/tmp"),
            files: vec![],
        };
        assert!(gpu.verify().is_err());
    }
}
