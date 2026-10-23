use anyhow::{Context, Result, ensure};
use glidepod_runtime::{Store, management::ClearStorage};
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires a running disposable GLIDEPOD_TEST_STORE; changes only the original test fixture"]
fn real_storage_permissions_and_notifications() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let data =
        PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing test store")?)
            .canonicalize()?;
    ensure!(
        data.starts_with(root.join(".cache")),
        "Use a disposable project-local store"
    );
    let store = Store::new(data)?;
    let g = store.guest()?;
    let package = "dev.glidepod.fixture";
    g.install(&root.join("target/android/glidepod-fixture.apk"), None)?;
    let overview = g.storage_overview()?;
    ensure!(
        overview.total_bytes > 0 && overview.available_bytes > 0,
        "Invalid volume statistics"
    );
    let before = g.app_details(package)?;
    ensure!(
        before
            .permissions
            .iter()
            .any(|p| p.name == "android.permission.POST_NOTIFICATIONS"),
        "Notification permission missing"
    );
    ensure!(
        g.clear_app_storage("com.android.settings", ClearStorage::Data)
            .is_err(),
        "System data cleanup accepted"
    );
    ensure!(
        g.set_permission(package, "android.permission.READ_CONTACTS", true)
            .is_err(),
        "Unrequested permission granted"
    );
    g.set_permission(package, "android.permission.CAMERA", true)?;
    g.set_permission(package, "android.permission.CAMERA", false)?;
    let w = g.open_app(package, 720, 1080)?;
    let launch = |extras: &str| -> Result<()> {
        g.developer_shell(&format!("am start -W --display {} -f 0x10008000 -n dev.glidepod.fixture/.MainActivity {extras}",w.info.display_id),false)?;
        Ok(())
    };
    launch("--ez seed_storage true")?;
    let seeded = g.app_details(package)?;
    ensure!(
        seeded.storage.as_ref().context("Missing sizes")?.data_bytes >= 2 * 1024 * 1024,
        "Seeded data missing from sizes"
    );
    g.clear_app_storage(package, ClearStorage::Cache)?;
    ensure!(
        g.developer_shell(
            "su 0 test -f /data/user/0/dev.glidepod.fixture/files/persistent.bin && echo PRESENT",
            false
        )?
        .trim()
            == "PRESENT",
        "Cache clearing removed user data"
    );
    ensure!(
        g.developer_shell(
            "su 0 test ! -f /data/user/0/dev.glidepod.fixture/cache/temporary.bin && echo REMOVED",
            false
        )?
        .trim()
            == "REMOVED",
        "Cache clearing did not remove cache"
    );
    let cached = g.app_details(package)?;
    g.clear_app_storage(package, ClearStorage::Data)?;
    ensure!(
        g.developer_shell(
            "su 0 test ! -f /data/user/0/dev.glidepod.fixture/files/persistent.bin && echo REMOVED",
            false
        )?
        .trim()
            == "REMOVED",
        "Data clearing retained private files"
    );
    ensure!(
        g.app_details(package)?.package_id == package,
        "Clear data uninstalled the app"
    );
    g.set_notification_access(true)?;
    struct Revoke(glidepod_runtime::guest::Guest);
    impl Drop for Revoke {
        fn drop(&mut self) {
            let _ = self.0.set_notification_access(false);
        }
    }
    let _revoke = Revoke(g.clone());
    let until = Instant::now() + Duration::from_secs(10);
    while !g.notifications()?.connected {
        ensure!(
            Instant::now() < until,
            "Notification listener not connected"
        );
        thread::sleep(Duration::from_millis(200));
    }
    g.set_permission(package, "android.permission.POST_NOTIFICATIONS", true)?;
    launch("--ez probe_provider true --es notification public")?;
    ensure!(
        g.developer_shell(
            "su 0 cat /data/user/0/dev.glidepod.fixture/files/provider-probe.txt",
            false
        )?
        .trim()
            == "DENIED",
        "Ordinary Android app could read notification provider"
    );
    thread::sleep(Duration::from_millis(300));
    let public = g.notifications()?;
    let notification = public
        .items
        .iter()
        .find(|n| n.package_id == package)
        .context("Missing public notification")?;
    ensure!(
        notification.body.contains("中文") && !notification.redacted,
        "Public notification lost text"
    );
    let stable_id = notification.host_id(g.endpoints.epoch);
    let key = notification.key.clone();
    launch("--es notification update")?;
    thread::sleep(Duration::from_millis(300));
    let updated = g.notifications()?;
    let item = updated
        .items
        .iter()
        .find(|n| n.key == key)
        .context("Notification update changed identity")?;
    ensure!(
        item.title.ends_with("update") && item.host_id(g.endpoints.epoch) == stable_id,
        "Update did not retain native identity"
    );
    launch("--es notification private")?;
    launch("--es notification secret")?;
    thread::sleep(Duration::from_millis(300));
    let redaction = g.notifications()?;
    ensure!(
        redaction
            .items
            .iter()
            .any(|n| n.package_id == package && n.redacted && n.body == "New notification"),
        "Private content was not redacted"
    );
    ensure!(
        !redaction.items.iter().any(|n| n.title.ends_with("secret")),
        "Secret notification leaked"
    );
    g.dismiss_notification(&key)?;
    thread::sleep(Duration::from_millis(300));
    ensure!(
        !g.notifications()?.items.iter().any(|n| n.key == key),
        "Dismiss did not reach Android"
    );
    g.set_permission(package, "android.permission.POST_NOTIFICATIONS", false)?;
    launch("--es notification denied")?;
    thread::sleep(Duration::from_millis(300));
    ensure!(
        !g.notifications()?
            .items
            .iter()
            .any(|n| n.package_id == package),
        "Denied app still posted notifications"
    );
    g.set_notification_access(false)?;
    ensure!(
        !g.notification_access_enabled()?,
        "Android notification access was not revoked"
    );
    let out = root.join("evidence/g4/management");
    fs::create_dir_all(&out)?;
    let report = serde_json::json!({"passed":true,"epoch":g.endpoints.epoch,"volume":overview,"storage_seeded":seeded.storage,"storage_after_cache_clear":cached.storage,"checks":["Android storage statistics","per-app cache clear preserves data","data clear preserves installation","system app protection","unrequested permission rejection","runtime permission grant/revoke","notification access enable/revoke","ordinary app provider access denied","public Unicode notification","stable identity on update","private redaction and secret filtering","Android dismissal","POST_NOTIFICATIONS denial enforced"],"native_notification_banner_verified":false,"physical_ui_verified":false});
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
