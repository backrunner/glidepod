//! Real guest acceptance; explicitly opt in with a disposable, already-running store.
use anyhow::{Context, Result, ensure};
use glidepod_runtime::{
    Store,
    display::{AppWindow, Frame},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
fn capture(window: &AppWindow, path: PathBuf, after: u64) -> Result<Frame> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(frame) = window.frame(after)? {
            let mut f = std::io::BufWriter::new(fs::File::create(path)?);
            write!(f, "P6\n{} {}\n255\n", frame.width, frame.height)?;
            for rgba in frame.rgba.as_chunks::<4>().0 {
                f.write_all(&rgba[..3])?;
            }
            f.flush()?;
            return Ok(frame);
        }
        ensure!(Instant::now() < deadline, "No new frame");
        thread::sleep(Duration::from_millis(100));
    }
}
#[test]
#[ignore = "requires GLIDEPOD_TEST_STORE and a running disposable Android VM"]
fn real_guest_app_lifecycle() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let data =
        PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing test store")?)
            .canonicalize()?;
    ensure!(
        data.starts_with(root.join(".cache")),
        "Use a disposable project-local test store"
    );
    let store = Store::new(data)?;
    let guest = store.guest()?;
    let out = root.join("evidence/g2").join(
        if store.runtime_status()?.effective_renderer == Some(glidepod_core::Renderer::Metal) {
            "metal-session"
        } else {
            "software-session"
        },
    );
    fs::create_dir_all(&out)?;
    let apk = root.join("target/android/glidepod-fixture.apk");
    let receipt = guest.install(&apk, None)?;
    ensure!(receipt.android_result == "Success", "Install failed");
    guest.install(&apk, Some(&receipt.sha256))?;
    ensure!(
        guest.install(&apk, Some(&"0".repeat(64))).is_err(),
        "Changed APK accepted"
    );
    let unsigned = root.join("target/android/fixtures/unsigned.apk");
    let rejected = guest.install(&unsigned, None).unwrap_err().to_string();
    ensure!(
        rejected.contains("INSTALL_PARSE_FAILED_NO_CERTIFICATES"),
        "Unexpected unsigned APK result: {rejected}"
    );
    ensure!(
        guest
            .catalog()?
            .iter()
            .any(|a| a.package_id == "dev.glidepod.fixture"),
        "Catalog missing fixture"
    );
    ensure!(
        guest.uninstall("com.android.settings").is_err(),
        "System package removal allowed"
    );
    let c = store.load()?;
    store.update(c.revision, |c| c.developer.root_requested = false)?;
    ensure!(
        guest.developer_shell("id -u", true).is_err(),
        "Root shortcut bypassed"
    );
    let c = store.load()?;
    store.update(c.revision, |c| c.developer.root_requested = true)?;
    ensure!(
        guest.developer_shell("id -u", true)?.trim() == "0",
        "Guest root not verified"
    );
    let c = store.load()?;
    store.update(c.revision, |c| c.developer.root_requested = false)?;
    let a = guest.open_app("dev.glidepod.fixture", 720, 1080)?;
    ensure!(
        guest.open_app("dev.glidepod.fixture", 720, 1080).is_err(),
        "Duplicate app window admitted"
    );
    let b = guest.open_app("com.android.settings", 800, 1000)?;
    ensure!(
        a.info.display_id != b.info.display_id && a.info.scanout != b.info.scanout,
        "Displays not isolated"
    );
    thread::sleep(Duration::from_secs(2));
    a.tap(180, 160)?;
    a.text("Mac keyboard test · 中文 😀 100%")?;
    a.edit_action("select_all")?;
    ensure!(
        a.edit_action("copy")?.as_deref() == Some("Mac keyboard test · 中文 😀 100%"),
        "Unicode selection copy did not round-trip"
    );
    a.key(22)?;
    let before = capture(&a, out.join("input-before.ppm"), 0)?;
    a.tap(360, 235)?;
    thread::sleep(Duration::from_millis(300));
    let after = capture(&a, out.join("input-after.ppm"), before.sequence)?;
    ensure!(before.rgba != after.rgba, "Tap did not change the frame");
    capture(&b, out.join("second-display.ppm"), 0)?;
    ensure!(
        a.task_count()? == 1 && b.task_count()? == 1,
        "Owned tasks missing"
    );
    drop(a);
    let activities = guest.developer_shell("dumpsys activity activities", false)?;
    ensure!(
        !activities.contains("dev.glidepod.fixture/.MainActivity"),
        "Closed fixture task survived display removal"
    );
    ensure!(
        b.task_count()? == 1,
        "Closing one window removed another app's task"
    );
    drop(b);
    if guest
        .catalog()?
        .iter()
        .any(|a| a.package_id == "dev.glidepod.widgets")
    {
        guest.uninstall("dev.glidepod.widgets")?;
    }
    guest.install(&root.join("target/android/glidepod-widgets.apk"), None)?;
    let widgets = guest.open_app("dev.glidepod.widgets", 720, 1080)?;
    thread::sleep(Duration::from_secs(1));
    widgets.tap(360, 60)?;
    thread::sleep(Duration::from_secs(1));
    capture(&widgets, out.join("widget-picker.ppm"), 0)?;
    widgets.tap(280, 322)?;
    thread::sleep(Duration::from_secs(1));
    capture(&widgets, out.join("widget-bound.ppm"), 0)?;
    drop(widgets);
    let restored = guest.open_app("dev.glidepod.widgets", 720, 1080)?;
    thread::sleep(Duration::from_secs(1));
    capture(&restored, out.join("widget-restored.ppm"), 0)?;
    let restored = std::sync::Arc::new(restored);
    let mut input = restored.open_input()?;
    restored.key(4)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        input.send(glidepod_runtime::input::InputEvent::Ping)?;
        if !input.task_present {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Back did not report task completion"
        );
        thread::sleep(Duration::from_millis(100));
    }
    drop(input);
    drop(restored);
    guest.uninstall("dev.glidepod.fixture")?;
    ensure!(
        !guest
            .catalog()?
            .iter()
            .any(|a| a.package_id == "dev.glidepod.fixture"),
        "Uninstall did not update catalog"
    );
    guest.install(&apk, None)?;
    guest.catalog()?;
    let report = serde_json::json!({"epoch":guest.endpoints.epoch,"renderer":store.runtime_status()?.effective_renderer,"checks":["install","update","reviewed-hash rejection","unsigned APK rejection","system uninstall rejection","catalog and icon extraction","root shortcut policy and uid","simultaneous independent displays","duplicate app window rejection","close removes only owned task","Back reports task completion","display-targeted text and pointer input","widget picker, binding and restore captures","Unicode selection and clipboard round-trip","uninstall and reinstall"],"native_ui_buttons_verified":false,"game_compatibility_qualified":false});
    fs::write(
        out.join("app-session.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
