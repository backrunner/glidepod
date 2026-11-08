//! Reversible optional-app defaults, applied before declaring the guest ready.
use crate::{adb::Adb, guest::resource};
use anyhow::{Result, ensure};
use std::{path::Path, time::Duration};

pub(crate) fn prepare(vsock: &Path, minimal: bool) -> Result<()> {
    Adb::shell(
        vsock,
        "mkdir -p /data/local/tmp/glidepod",
        Duration::from_secs(10),
    )?
    .text()?;
    let remote = "/data/local/tmp/glidepod/system-apps-bridge.jar";
    Adb::push(vsock, &resource("glidepod-bridge.jar")?, remote)?;
    let mode = if minimal { "minimal" } else { "full" };
    let result = Adb::shell(
        vsock,
        &format!("su 0 env CLASSPATH={remote} app_process / dev.glidepod.bridge.SystemApps {mode}"),
        Duration::from_secs(30),
    )?
    .text()?;
    let report: serde_json::Value = serde_json::from_str(result.trim())?;
    ensure!(
        report["version"] == 1 && report["mode"] == mode,
        "Invalid built-in app policy reply"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn old_configurations_get_reversible_minimal_defaults() {
        let mut value = serde_json::to_value(crate::Configuration::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("minimal_builtin_apps");
        let old: crate::Configuration = serde_json::from_value(value.clone()).unwrap();
        assert!(old.minimal_builtin_apps);
        value["minimal_builtin_apps"] = false.into();
        let full: crate::Configuration = serde_json::from_value(value).unwrap();
        assert!(!full.minimal_builtin_apps);
    }
    #[test]
    fn desktop_policy_never_lists_compatibility_providers_or_services() {
        let source =
            include_str!("../../../android/bridge/src/dev/glidepod/bridge/SystemApps.java");
        let list = source
            .split("private static final String[] OPTIONAL = {")
            .nth(1)
            .unwrap()
            .split("};")
            .next()
            .unwrap();
        for essential in [
            "com.android.phone",
            "com.android.providers.contacts",
            "com.android.providers.calendar",
            "com.android.webview",
            "com.android.contactspicker",
            "com.android.photopicker",
            "com.android.documentsui",
            "com.android.providers.downloads",
            "com.android.permissioncontroller",
            "com.android.systemui",
            "com.google.android.gms",
            "com.android.camera2",
        ] {
            assert!(
                !list.contains(&format!("\"{essential}\"")),
                "Essential package {essential} must remain available"
            );
        }
    }
}
