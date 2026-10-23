use crate::{
    adb::quote,
    guest::{Guest, package_id, resource},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidNotification {
    pub key: String,
    pub package_id: String,
    pub app_label: String,
    pub title: String,
    pub body: String,
    pub posted_ms: u64,
    pub clearable: bool,
    pub redacted: bool,
    pub alerting: bool,
}
impl AndroidNotification {
    pub fn host_id(&self, epoch: u64) -> String {
        format!("glidepod.{epoch}.{:x}", Sha256::digest(self.key.as_bytes()))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSnapshot {
    pub connected: bool,
    pub items: Vec<AndroidNotification>,
}
impl Guest {
    /// Called only after an explicit enable action. Disabling revokes Android listener access.
    pub fn set_notification_access(&self, enabled: bool) -> Result<()> {
        if enabled {
            self.install(&resource("glidepod-notifications.apk")?, None)?;
        }
        self.shell(&format!(
            "cmd notification {} dev.glidepod.notifications/.Listener 0",
            if enabled {
                "allow_listener"
            } else {
                "disallow_listener"
            }
        ))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while self.notification_access_enabled()? != enabled {
            ensure!(
                std::time::Instant::now() < deadline,
                "Android did not apply notification access"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Ok(())
    }
    pub fn notifications(&self) -> Result<NotificationSnapshot> {
        let out = self.provider("dev.glidepod.notifications.bridge", "snapshot", None)?;
        ensure!(
            out.len() <= 2 * 1024 * 1024,
            "Notification snapshot exceeds limit"
        );
        let snapshot: NotificationSnapshot = serde_json::from_str(out.trim())?;
        ensure!(snapshot.items.len() <= 256, "Too many notifications");
        for n in &snapshot.items {
            package_id(&n.package_id)?;
            ensure!(
                n.key.len() <= 4096 && n.title.len() <= 1024 && n.body.len() <= 8192,
                "Oversized notification"
            );
        }
        Ok(snapshot)
    }
    pub fn dismiss_notification(&self, key: &str) -> Result<()> {
        ensure!(
            !key.contains('\0') && key.len() <= 4096,
            "Invalid notification key"
        );
        self.provider("dev.glidepod.notifications.bridge", "dismiss", Some(key))?;
        Ok(())
    }
    pub fn notification_access_enabled(&self) -> Result<bool> {
        let value = self.shell("settings get secure enabled_notification_listeners")?;
        Ok(value.trim().split(':').any(|c| {
            c == "dev.glidepod.notifications/.Listener"
                || c == "dev.glidepod.notifications/dev.glidepod.notifications.Listener"
        }))
    }
    pub fn open_android_app_settings(&self, package: &str, display: u32) -> Result<()> {
        package_id(package)?;
        let result=self.shell(&format!("am start -W --display {display} -a android.settings.APPLICATION_DETAILS_SETTINGS -d {}",quote(&format!("package:{package}"))))?;
        ensure!(
            !result.contains("Error:"),
            "Android could not open app settings: {result}"
        );
        Ok(())
    }
}
