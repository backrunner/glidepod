use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
/// Creates an explicitly requested, per-user launch entry. Never replaces unrelated bundles.
pub fn create(package: &str, label: &str, icon: Option<&Path>) -> Result<PathBuf> {
    ensure!(
        package.len() <= 255
            && package.contains('.')
            && package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_'),
        "Invalid package ID"
    );
    let root = PathBuf::from(std::env::var_os("HOME").context("HOME unavailable")?)
        .join("Applications/GlidePod Apps");
    fs::create_dir_all(&root)?;
    let path = root.join(format!("{package}.app"));
    if path.exists() {
        let info = plist::Value::from_file(path.join("Contents/Info.plist"))?;
        ensure!(
            info.as_dictionary()
                .and_then(|d| d.get("GlidePodPackage"))
                .and_then(plist::Value::as_string)
                == Some(package),
            "A different app already occupies this path"
        );
        return Ok(path);
    }
    let exe = std::env::current_exe()?;
    let source = exe.with_file_name("glidepod-launcher");
    ensure!(
        source.is_file(),
        "Package GlidePod with its launcher helper first"
    );
    let temporary = tempfile::Builder::new()
        .prefix(".entry-")
        .tempdir_in(&root)?;
    let contents = temporary.path().join("Contents");
    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("Resources"))?;
    fs::copy(source, contents.join("MacOS/launcher"))?;
    let mut info = plist::Dictionary::new();
    for (key, value) in [
        ("CFBundleName", label),
        ("CFBundleDisplayName", label),
        ("CFBundleExecutable", "launcher"),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleVersion", "1"),
        ("GlidePodPackage", package),
        ("LSMinimumSystemVersion", "15.0"),
    ] {
        info.insert(key.into(), value.into());
    }
    info.insert(
        "CFBundleIdentifier".into(),
        format!("dev.glidepod.app.{package}").into(),
    );
    if let Some(icon) = icon {
        fs::copy(icon, contents.join("Resources/AppIcon.png"))?;
        info.insert("CFBundleIconFile".into(), "AppIcon.png".into());
    }
    plist::Value::Dictionary(info).to_file_xml(contents.join("Info.plist"))?;
    let signed = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(temporary.path())
        .output()?;
    ensure!(
        signed.status.success(),
        "Cannot sign app entry: {}",
        String::from_utf8_lossy(&signed.stderr)
    );
    fs::rename(temporary.path(), &path)?;
    Ok(path)
}
