use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
/// Creates or refreshes a managed per-user Dock/Finder entry.
/// Never replaces unrelated bundles; old one-shot launchers migrate in place.
pub fn create(package: &str, label: &str, icon: Option<&Path>) -> Result<PathBuf> {
    let data = std::env::var_os("GLIDEPOD_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or(
            PathBuf::from(std::env::var_os("HOME").context("HOME unavailable")?)
                .join("Library/Application Support/GlidePod"),
        );
    create_for_store(package, label, icon, &data)
}

pub fn create_for_store(
    package: &str,
    label: &str,
    icon: Option<&Path>,
    data: &Path,
) -> Result<PathBuf> {
    let library = Library::new(data)?;
    let _lock = library.lock()?;
    let path = create_at(
        &library.root,
        &library.source,
        &library.host,
        &library.data,
        package,
        label,
        icon,
    )?;
    register(&path)?;
    index(&path)?;
    // Publishing the Applications-folder link is reconciled separately. A
    // protected /Applications directory must not prevent opening an app window.
    Ok(path)
}

struct Library {
    root: PathBuf,
    source: PathBuf,
    host: String,
    data: String,
}
impl Library {
    fn new(data: &Path) -> Result<Self> {
        let exe = std::env::current_exe()?;
        let host = exe
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME unavailable")?);
        Ok(Self {
            root: home.join("Applications/GlidePod Apps"),
            source: exe.with_file_name("glidepod-launcher"),
            host,
            data: data.canonicalize()?.to_string_lossy().into_owned(),
        })
    }
    fn lock(&self) -> Result<fs::File> {
        fs::create_dir_all(&self.root)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".launchers.lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }
}

pub struct AppEntry<'a> {
    pub package: &'a str,
    pub label: &'a str,
    pub icon: Option<&'a Path>,
    pub in_library: bool,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SyncReport {
    pub available: usize,
    pub removed: usize,
    pub warnings: Vec<String>,
}
impl SyncReport {
    pub fn warning(&self) -> Option<String> {
        (!self.warnings.is_empty()).then(|| {
            format!(
                "Some Mac app entries could not be synced: {}",
                self.warnings.join("; ")
            )
        })
    }
}

/// `prune` is true only for a complete, freshly fetched Android catalog.
/// Cached catalogs can create/update entries while offline, but never remove them.
pub fn synchronize(apps: &[AppEntry<'_>], data: &Path, prune: bool) -> Result<SyncReport> {
    let library = Library::new(data)?;
    let _lock = library.lock()?;
    let mut report = sync_at(&library, apps, prune)?;
    for path in managed_bundles(&library.root)? {
        if belongs_to_store(&path, &library.data)
            && let Err(e) = register(&path)
        {
            report.warnings.push(e.to_string());
        }
    }
    if let Err(e) = index(&library.root) {
        report.warnings.push(e.to_string());
    }
    if let Err(e) = expose_in_applications(&library.root) {
        report.warnings.push(e.to_string());
    }
    Ok(report)
}

fn sync_at(library: &Library, apps: &[AppEntry<'_>], prune: bool) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let existing = managed_bundles(&library.root)?;
    for app in apps {
        let previously_exported = existing.iter().any(|p| {
            managed_package(p).as_deref() == Some(app.package) && belongs_to_store(p, &library.data)
        });
        if !app.in_library && !previously_exported {
            continue;
        }
        match create_at(
            &library.root,
            &library.source,
            &library.host,
            &library.data,
            app.package,
            app.label,
            app.icon,
        ) {
            Ok(_) => report.available += 1,
            Err(e) => report.warnings.push(format!("{}: {e:#}", app.label)),
        }
    }
    if prune {
        for path in existing {
            if belongs_to_store(&path, &library.data)
                && let Some(package) = managed_package(&path)
                && !apps.iter().any(|a| a.package == package)
            {
                match fs::remove_dir_all(&path) {
                    Ok(()) => report.removed += 1,
                    Err(e) => report
                        .warnings
                        .push(format!("Remove {}: {e}", path.display())),
                }
            }
        }
    }
    Ok(report)
}

fn managed_package(path: &Path) -> Option<String> {
    if path.is_symlink() || !path.is_dir() {
        return None;
    }
    let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok()?;
    let info = info.as_dictionary()?;
    let package = info.get("GlidePodPackage")?.as_string()?;
    if let Some(id) = info
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string)
        && id != format!("dev.glidepod.app.{package}")
    {
        return None;
    }
    Some(package.into())
}
fn managed_bundles(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "app") && managed_package(&path).is_some() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}
fn belongs_to_store(path: &Path, data: &str) -> bool {
    let Ok(info) = plist::Value::from_file(path.join("Contents/Info.plist")) else {
        return false;
    };
    let stored = info
        .as_dictionary()
        .and_then(|d| d.get("GlidePodDataDirectory"))
        .and_then(plist::Value::as_string)
        .unwrap_or("");
    if stored.is_empty() {
        // Legacy entries without an explicit store belong to the normal user store.
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .is_some_and(|p| {
                p.join("Library/Application Support/GlidePod")
                    .to_string_lossy()
                    == data
            })
    } else {
        stored == data
    }
}

fn filename(label: &str) -> String {
    let mut name = String::new();
    for c in label.chars() {
        let c = if c.is_control() || matches!(c, '/' | ':' | '\\') {
            '-'
        } else {
            c
        };
        if name.len() + c.len_utf8() > 160 {
            break;
        }
        name.push(c);
    }
    let name = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if name.is_empty() {
        "Android App".into()
    } else {
        name.into()
    }
}
fn entry_path(root: &Path, package: &str, label: &str) -> Result<PathBuf> {
    let name = filename(label);
    for suffix in [
        String::new(),
        format!(
            " ({})",
            &hex::encode(Sha256::digest(package.as_bytes()))[..8]
        ),
    ] {
        let path = root.join(format!("{name}{suffix}.app"));
        if fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            || managed_package(&path).as_deref() == Some(package)
        {
            return Ok(path);
        }
    }
    anyhow::bail!("An unrelated app occupies the Mac entry path")
}

fn expose_in_applications(root: &Path) -> Result<()> {
    expose_at(root, Path::new("/Applications/GlidePod Apps"))
}
fn expose_at(root: &Path, link: &Path) -> Result<()> {
    match fs::symlink_metadata(link) {
        Ok(_) => ensure!(
            fs::read_link(link).is_ok_and(|target| target == root),
            "{} is occupied; Mac app entries are available in {}",
            link.display(),
            root.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(root, link)
                .with_context(|| format!("Cannot add {} to Applications", root.display()))?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn index(path: &Path) -> Result<()> {
    let output = Command::new("/usr/bin/mdimport")
        .arg("-i")
        .arg(path)
        .output()?;
    ensure!(
        output.status.success(),
        "Cannot request Spotlight indexing for {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
#[cfg(target_os = "macos")]
fn register(path: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn glidepod_register_application(path: *const std::ffi::c_char) -> i32;
    }
    let path = CString::new(path.as_os_str().as_bytes())?;
    let status = unsafe { glidepod_register_application(path.as_ptr()) };
    ensure!(
        status == 0,
        "Cannot register Mac app entry (Launch Services status {status})"
    );
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn register(_: &Path) -> Result<()> {
    Ok(())
}

fn create_at(
    root: &Path,
    source: &Path,
    host: &str,
    data: &str,
    package: &str,
    label: &str,
    icon: Option<&Path>,
) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    ensure!(package.len() <= 255 && package.contains('.') && package.split('.').all(|s|
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')), "Invalid package ID");
    let path = entry_path(root, package, label)?;
    let old = managed_bundles(root)?
        .into_iter()
        .find(|p| managed_package(p).as_deref() == Some(package));
    if let Some(old) = &old {
        let info = plist::Value::from_file(old.join("Contents/Info.plist"))?;
        let recorded = info
            .as_dictionary()
            .and_then(|d| d.get("GlidePodDataDirectory"))
            .and_then(plist::Value::as_string)
            .unwrap_or("");
        ensure!(
            recorded.is_empty() || recorded == data,
            "This Mac entry belongs to a different Android data directory"
        );
    }
    let existing = old
        .as_ref()
        .map(|p| plist::Value::from_file(p.join("Contents/Info.plist")))
        .transpose()?;
    ensure!(
        source.is_file(),
        "Package GlidePod with its launcher helper first"
    );
    let socket = crate::dock::socket_path(package)?;
    let old_icon = old
        .as_ref()
        .unwrap_or(&path)
        .join("Contents/Resources/AppIcon.png");
    let icon = icon.or_else(|| old_icon.is_file().then_some(old_icon.as_path()));
    let helper_hash = hex::encode(Sha256::digest(fs::read(source)?));
    let icon_hash = icon
        .map(fs::read)
        .transpose()?
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .unwrap_or_default();
    let socket_text = socket.to_string_lossy();
    let current = existing.as_ref().and_then(plist::Value::as_dictionary);
    if old.as_ref() == Some(&path)
        && current.is_some_and(|d| {
            [
                ("CFBundleDisplayName", label),
                ("GlidePodEntryVersion", "3"),
                ("GlidePodHelperHash", &helper_hash),
                ("GlidePodIconHash", &icon_hash),
                ("GlidePodDockSocket", &socket_text),
                ("GlidePodHostBundle", host),
                ("GlidePodDataDirectory", data),
            ]
            .iter()
            .all(|(key, value)| d.get(key).and_then(plist::Value::as_string) == Some(*value))
        })
    {
        return Ok(path);
    }
    let temporary = tempfile::Builder::new()
        .prefix(".entry-")
        .tempdir_in(root)?;
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
        ("CFBundleVersion", "3"),
        ("GlidePodEntryVersion", "3"),
        ("GlidePodPackage", package),
        ("LSMinimumSystemVersion", "15.0"),
        ("GlidePodHelperHash", &helper_hash),
        ("GlidePodIconHash", &icon_hash),
        ("GlidePodDockSocket", &socket_text),
        ("GlidePodHostBundle", host),
        ("GlidePodDataDirectory", data),
    ] {
        info.insert(key.into(), value.into());
    }
    info.insert("LSMultipleInstancesProhibited".into(), true.into());
    info.insert("NSHighResolutionCapable".into(), true.into());
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
    // Build/sign completely before touching an existing entry, and roll back if
    // publishing the replacement fails. The running helper keeps its old inode.
    let previous = tempfile::Builder::new()
        .prefix(".previous-")
        .tempdir_in(root)?;
    let saved = previous.path().join("entry.app");
    if let Some(old) = &old {
        fs::rename(old, &saved)?;
    }
    if let Err(error) = fs::rename(temporary.path(), &path) {
        if saved.exists()
            && let Err(rollback) = fs::rename(&saved, old.as_ref().unwrap_or(&path))
        {
            // Do not let TempDir delete the only surviving original if another
            // writer or filesystem failure prevents restoration.
            let preserved = previous.keep();
            return Err(error).with_context(|| {
                format!(
                    "Cannot restore app entry ({rollback}); original preserved at {}",
                    preserved.join("entry.app").display()
                )
            });
        }
        return Err(error.into());
    }
    Ok(path)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn upgrades_managed_launchers_and_preserves_unrelated_bundles() {
        let root = tempfile::tempdir().unwrap();
        let source = Path::new("/usr/bin/true");
        let package = "dev.glidepod.docktest";
        let path = create_at(
            root.path(),
            source,
            "/Example.app",
            "/test-store",
            package,
            "First",
            None,
        )
        .unwrap();
        let plist = path.join("Contents/Info.plist");
        let first = fs::read(&plist).unwrap();
        let same = create_at(
            root.path(),
            source,
            "/Example.app",
            "/test-store",
            package,
            "First",
            None,
        )
        .unwrap();
        assert_eq!(path, same);
        assert_eq!(first, fs::read(&plist).unwrap());
        // Old one-shot bundles had only the package ownership marker.
        let mut info = plist::Dictionary::new();
        info.insert("GlidePodPackage".into(), package.into());
        plist::Value::Dictionary(info).to_file_xml(&plist).unwrap();
        let updated = create_at(
            root.path(),
            source,
            "/Example.app",
            "/test-store",
            package,
            "Renamed",
            None,
        )
        .unwrap();
        assert!(!path.exists());
        assert_eq!(updated.file_name().unwrap(), "Renamed.app");
        let plist = updated.join("Contents/Info.plist");
        let refreshed = plist::Value::from_file(&plist).unwrap();
        let refreshed = refreshed.as_dictionary().unwrap();
        assert_eq!(
            refreshed["CFBundleDisplayName"].as_string(),
            Some("Renamed")
        );
        assert_eq!(
            refreshed["GlidePodDataDirectory"].as_string(),
            Some("/test-store")
        );
        assert!(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict"])
                .arg(&updated)
                .status()
                .unwrap()
                .success()
        );
        let mut unrelated = plist::Dictionary::new();
        unrelated.insert("GlidePodPackage".into(), "another.app".into());
        plist::Value::Dictionary(unrelated)
            .to_file_xml(&plist)
            .unwrap();
        let before = fs::read(&plist).unwrap();
        let collision = create_at(root.path(), source, "", "", package, "Renamed", None).unwrap();
        assert_ne!(collision, updated);
        assert_eq!(before, fs::read(&plist).unwrap());
    }

    #[test]
    fn sync_handles_names_and_only_prunes_owned_entries_from_a_live_catalog() {
        let root = tempfile::tempdir().unwrap();
        let library = Library {
            root: root.path().to_owned(),
            source: "/usr/bin/true".into(),
            host: "/Example.app".into(),
            data: "/store-a".into(),
        };
        let apps = [
            AppEntry {
                package: "dev.first",
                label: "笔记",
                icon: None,
                in_library: true,
            },
            AppEntry {
                package: "dev.second",
                label: "笔记",
                icon: None,
                in_library: true,
            },
            AppEntry {
                package: "com.android.hidden",
                label: "Hidden",
                icon: None,
                in_library: false,
            },
        ];
        assert_eq!(sync_at(&library, &apps, true).unwrap().available, 2);
        let entries = managed_bundles(root.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(root.path().join("笔记.app").exists());
        assert_eq!(sync_at(&library, &[], false).unwrap().removed, 0);
        let other_store = create_at(
            root.path(),
            &library.source,
            "",
            "/store-b",
            "dev.other",
            "Other",
            None,
        )
        .unwrap();
        let unrelated = root.path().join("Unrelated.app");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), b"unmanaged").unwrap();
        let link = root.path().join("Link.app");
        std::os::unix::fs::symlink(&other_store, &link).unwrap();
        assert_eq!(sync_at(&library, &[], true).unwrap().removed, 2);
        assert!(other_store.exists());
        assert_eq!(fs::read(unrelated.join("keep")).unwrap(), b"unmanaged");
        assert!(link.is_symlink());
    }

    #[test]
    fn invalid_names_failed_updates_and_store_changes_preserve_existing_data() {
        assert_eq!(filename(" . . Hidden . . "), "Hidden");
        let root = tempfile::tempdir().unwrap();
        let source = Path::new("/usr/bin/true");
        let path = create_at(
            root.path(),
            source,
            "",
            "/store-a",
            "dev.safe",
            "../恶意/:\\名称",
            None,
        )
        .unwrap();
        assert_eq!(path.parent(), Some(root.path()));
        let before = fs::read(path.join("Contents/Info.plist")).unwrap();
        assert!(
            create_at(
                root.path(),
                Path::new("/missing-helper"),
                "",
                "/store-a",
                "dev.safe",
                "New",
                None
            )
            .is_err()
        );
        assert!(create_at(root.path(), source, "", "/store-b", "dev.safe", "New", None).is_err());
        assert_eq!(before, fs::read(path.join("Contents/Info.plist")).unwrap());
        assert!(create_at(root.path(), source, "", "", "../../escape", "Test", None).is_err());
        let link = root.path().join("Applications entry");
        expose_at(root.path(), &link).unwrap();
        expose_at(root.path(), &link).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), root.path());
        let occupied = root.path().join("occupied");
        fs::create_dir(&occupied).unwrap();
        assert!(expose_at(root.path(), &occupied).is_err());
        assert!(occupied.is_dir());
    }
}
