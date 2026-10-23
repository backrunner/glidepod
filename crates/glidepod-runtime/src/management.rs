//! Android-reported storage and runtime permissions, with explicit per-package mutations.
use crate::{
    adb::quote,
    guest::{Guest, package_id},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStorage {
    pub app_bytes: u64,
    pub data_bytes: u64,
    pub cache_bytes: u64,
}
impl AppStorage {
    /// Android includes cache in data. Do not count it twice.
    pub fn user_data_bytes(&self) -> u64 {
        self.data_bytes.saturating_sub(self.cache_bytes)
    }
    pub fn total_bytes(&self) -> u64 {
        self.app_bytes.saturating_add(self.data_bytes)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePermission {
    pub name: String,
    pub label: String,
    pub granted: bool,
    pub fixed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDetails {
    pub package_id: String,
    pub label: String,
    pub version: String,
    pub uid: u32,
    pub system: bool,
    pub shared_uid: bool,
    pub storage: Option<AppStorage>,
    pub storage_error: Option<String>,
    pub permissions: Vec<RuntimePermission>,
}
impl AppDetails {
    fn mutable(&self) -> Result<()> {
        ensure!(
            !self.system && self.uid >= 10000,
            "System apps cannot be changed here; use Android Settings"
        );
        ensure!(
            !self.shared_uid,
            "This app shares its Android identity; use Android Settings"
        );
        ensure!(
            !matches!(
                self.package_id.as_str(),
                "dev.glidepod.input" | "dev.glidepod.widgets" | "dev.glidepod.notifications"
            ),
            "GlidePod helpers cannot be changed here"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageOverview {
    pub total_bytes: u64,
    pub available_bytes: u64,
}
#[derive(Debug, Clone, Copy)]
pub enum ClearStorage {
    Cache,
    Data,
}
impl Guest {
    pub fn app_details(&self, package: &str) -> Result<AppDetails> {
        package_id(package)?;
        let out = self.bridge("dev.glidepod.bridge.Management", &["details", package])?;
        let details: AppDetails = serde_json::from_str(out.trim())?;
        ensure!(
            details.package_id == package && details.permissions.len() <= 256,
            "Invalid app details response"
        );
        Ok(details)
    }
    pub fn storage_overview(&self) -> Result<StorageOverview> {
        let out = self.bridge("dev.glidepod.bridge.Management", &["storage"])?;
        let s: StorageOverview = serde_json::from_str(out.trim())?;
        ensure!(
            s.available_bytes <= s.total_bytes,
            "Invalid Android storage statistics"
        );
        Ok(s)
    }
    pub fn clear_app_storage(&self, package: &str, kind: ClearStorage) -> Result<AppDetails> {
        self.app_details(package)?.mutable()?;
        let option = match kind {
            ClearStorage::Cache => "--cache-only ",
            ClearStorage::Data => "",
        };
        // Android 17 silently ignores shell cache deletion without INTERNAL_DELETE_CACHE_FILES.
        // The registered userdebug adapter has verified guest root; a rootless image needs a
        // platform-signed management service. Never substitute a global cache trim or rm.
        let prefix = if matches!(kind, ClearStorage::Cache) {
            ensure!(
                self.store.runtime_status()?.effective_root,
                "Cache clearing needs the privileged Android management service; use Android Settings"
            );
            "su 0 "
        } else {
            ""
        };
        let result = self.shell(&format!(
            "timeout 20 {prefix}pm clear --user 0 {option}{}",
            quote(package)
        ))?;
        ensure!(
            result.trim() == "Success",
            "Android could not clear storage: {result}"
        );
        self.app_details(package)
    }
    pub fn set_permission(
        &self,
        package: &str,
        permission: &str,
        grant: bool,
    ) -> Result<AppDetails> {
        let before = self.app_details(package)?;
        before.mutable()?;
        let p = before
            .permissions
            .iter()
            .find(|p| p.name == permission)
            .ok_or_else(|| anyhow::anyhow!("This app did not request that runtime permission"))?;
        ensure!(
            !p.fixed,
            "Android policy fixes this permission; use Android Settings"
        );
        let verb = if grant { "grant" } else { "revoke" };
        self.shell(&format!(
            "pm {verb} --user 0 {} {}",
            quote(package),
            quote(permission)
        ))?;
        let after = self.app_details(package)?;
        ensure!(
            after
                .permissions
                .iter()
                .any(|p| p.name == permission && p.granted == grant),
            "Android did not apply the requested permission change"
        );
        Ok(after)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destructive_controls_exclude_shared_system_and_helper_identities() {
        let mut d = AppDetails {
            package_id: "dev.example.app".into(),
            label: "Example".into(),
            version: "1".into(),
            uid: 10123,
            system: false,
            shared_uid: false,
            storage: None,
            storage_error: None,
            permissions: vec![],
        };
        assert!(d.mutable().is_ok());
        d.shared_uid = true;
        assert!(d.mutable().is_err());
        d.shared_uid = false;
        d.system = true;
        assert!(d.mutable().is_err());
        d.system = false;
        d.uid = 1000;
        assert!(d.mutable().is_err());
        d.uid = 10123;
        d.package_id = "dev.glidepod.notifications".into();
        assert!(d.mutable().is_err());
    }
    #[test]
    fn cache_is_not_counted_twice() {
        let s = AppStorage {
            app_bytes: 10,
            data_bytes: 25,
            cache_bytes: 5,
        };
        assert_eq!(s.total_bytes(), 35);
        assert_eq!(s.user_data_bytes(), 20);
    }
}
