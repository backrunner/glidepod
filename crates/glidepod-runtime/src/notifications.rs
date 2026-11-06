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
    pub ongoing: bool,
    pub group_summary: bool,
    pub group: String,
    pub user_id: i32,
    pub can_open: bool,
}
impl AndroidNotification {
    pub fn content_hash(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("Notification serialization"),
        ))
    }
    pub fn host_id(&self, epoch: u64) -> String {
        format!(
            "glidepod.{epoch}.{}",
            hex::encode(Sha256::digest(self.key.as_bytes()))
        )
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
        #[derive(Deserialize)]
        struct Page {
            connected: bool,
            items: Vec<AndroidNotification>,
            total: usize,
            revision: u64,
            consistent: bool,
        }
        for _ in 0..3 {
            let mut items = Vec::new();
            let mut revision = None;
            let mut expected = None;
            loop {
                let out = self.provider(
                    "dev.glidepod.notifications.bridge",
                    "snapshot",
                    Some(&items.len().to_string()),
                )?;
                ensure!(out.len() <= 1024 * 1024, "Notification page exceeds limit");
                let page: Page = serde_json::from_str(out.trim())?;
                if !page.connected {
                    return Ok(NotificationSnapshot {
                        connected: false,
                        items: vec![],
                    });
                }
                ensure!(
                    page.total <= 4096 && page.items.len() <= 64,
                    "Android exposes more notifications than the relay can safely retain"
                );
                if !page.consistent
                    || revision.is_some_and(|r| r != page.revision)
                    || expected.is_some_and(|n| n != page.total)
                {
                    break;
                }
                revision = Some(page.revision);
                expected = Some(page.total);
                for n in &page.items {
                    package_id(&n.package_id)?;
                    ensure!(
                        !n.key.is_empty()
                            && n.key.len() <= 4096
                            && n.app_label.len() <= 1024
                            && n.title.len() <= 1024
                            && n.body.len() <= 8192
                            && n.group.len() <= 4096,
                        "Oversized notification"
                    );
                }
                let empty = page.items.is_empty();
                items.extend(page.items);
                if items.len() == page.total {
                    let unique: std::collections::HashSet<_> =
                        items.iter().map(|n| &n.key).collect();
                    ensure!(
                        unique.len() == items.len(),
                        "Duplicate Android notification keys"
                    );
                    return Ok(NotificationSnapshot {
                        connected: true,
                        items,
                    });
                }
                ensure!(
                    !empty && items.len() < page.total,
                    "Invalid notification pagination"
                );
            }
        }
        anyhow::bail!("Android notifications changed during synchronization; retrying")
    }
    pub fn activate_notification(
        &self,
        notification: &AndroidNotification,
        display: u32,
    ) -> Result<()> {
        self.verify_epoch()?;
        ensure!(
            display > 0 && notification.can_open,
            "Notification has no Android click action"
        );
        let result=self.provider("dev.glidepod.notifications.bridge","open",Some(&serde_json::to_string(&serde_json::json!({"key":notification.key,"package_id":notification.package_id,"posted_ms":notification.posted_ms,"display":display}))?))?;
        let value: serde_json::Value = serde_json::from_str(result.trim())?;
        let token = value["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Notification click token missing"))?;
        ensure!(
            token.len() == 36 && token.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
            "Invalid notification click token"
        );
        let started=self.shell(&format!("am start -W --display {display} -f 0x18000000 -n dev.glidepod.notifications/.OpenNotification --es token {}",quote(token)))?;
        ensure!(
            !started.contains("Error:"),
            "Could not open the Android notification: {started}"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let out = self.provider(
                "dev.glidepod.notifications.bridge",
                "open_status",
                Some(token),
            )?;
            let status: serde_json::Value = serde_json::from_str(out.trim())?;
            if status["state"] == "done" {
                break;
            }
            ensure!(
                status["state"] != "failed",
                "Android notification: {}",
                status["error"]
            );
            ensure!(
                std::time::Instant::now() < deadline,
                "Android notification action timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
    pub fn dismiss_notification(&self, notification: &AndroidNotification) -> Result<()> {
        let key = &notification.key;
        ensure!(
            !key.contains('\0') && key.len() <= 4096,
            "Invalid notification key"
        );
        let request = serde_json::to_string(&serde_json::json!({
            "key": key, "package_id": notification.package_id, "posted_ms": notification.posted_ms
        }))?;
        self.provider(
            "dev.glidepod.notifications.bridge",
            "dismiss",
            Some(&request),
        )?;
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
