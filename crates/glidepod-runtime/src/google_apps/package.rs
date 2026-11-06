use super::*;
use std::{collections::HashSet, io::Read};

#[derive(Deserialize)]
pub(super) struct Source {
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
    pub sdk: u32,
}
pub(super) fn source() -> Source {
    serde_json::from_str(include_str!("../../../../engine/google-apps-lock.json"))
        .expect("Embedded GApps source")
}
pub(super) struct Payload {
    pub directory: PathBuf,
    pub files: Vec<String>,
    pub bytes: u64,
}
fn safe(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
pub(super) fn entries<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
) -> Result<Vec<(usize, String)>> {
    ensure!(zip.len() <= 4096, "Too many package entries");
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut selected = Vec::new();
    for index in 0..zip.len() {
        let f = zip.by_index(index)?;
        let raw_name = f.name()?;
        let name = raw_name.trim_end_matches('/');
        ensure!(
            safe(name) && names.insert(name.to_ascii_lowercase()),
            "Unsafe or duplicate package path"
        );
        ensure!(!f.encrypted(), "Encrypted package entry");
        let kind = f.unix_mode().unwrap_or(0) & 0o170000;
        ensure!(
            matches!(kind, 0 | 0o100000 | 0o040000),
            "Package links and special files are unsupported"
        );
        total = total
            .checked_add(f.size())
            .context("Package size overflow")?;
        ensure!(
            total <= 2 * 1024 * 1024 * 1024,
            "Expanded package exceeds 2 GiB"
        );
        if f.is_dir() {
            continue;
        }
        let Some(path) = name.strip_prefix("system/") else {
            continue;
        };
        if path.starts_with("addon.d/") {
            continue;
        }
        let parts: Vec<_> = path.split('/').collect();
        ensure!(
            parts.len() >= 3 && matches!(parts[0], "product" | "system_ext"),
            "Unsupported GApps layout"
        );
        ensure!(
            matches!(
                parts[1],
                "app" | "priv-app" | "apex" | "etc" | "framework" | "lib" | "lib64" | "overlay"
            ),
            "Unsupported GApps partition"
        );
        if matches!(parts[1], "app" | "priv-app")
            && ![
                "product/priv-app/Phonesky/",
                "product/priv-app/GooglePartnerSetup/",
                "system_ext/priv-app/GoogleServicesFramework/",
                "system_ext/priv-app/GoogleFeedback/",
            ]
            .iter()
            .any(|p| path.starts_with(p))
        {
            continue;
        }
        selected.push((index, path.to_string()));
    }
    let mut prop = String::new();
    zip.by_name("build.prop")?
        .take(65537)
        .read_to_string(&mut prop)?;
    ensure!(prop.len() <= 65536, "Oversized package properties");
    let mut properties = std::collections::HashMap::new();
    for line in prop
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
    {
        let (k, v) = line.split_once('=').context("Invalid package property")?;
        ensure!(
            properties.insert(k.trim(), v.trim()).is_none(),
            "Duplicate package property"
        );
    }
    ensure!(
        properties.get("arch") == Some(&"arm64") && properties.get("version") == Some(&"37"),
        "GApps requires Android 17 ARM64"
    );
    for required in [
        "product/priv-app/Phonesky/Phonesky.apk",
        "system_ext/priv-app/GoogleServicesFramework/GoogleServicesFramework.apk",
        "product/apex/com.google.android.gmssystem.prodvic.apex",
        "product/etc/permissions/privapp-permissions-google-product.xml",
        "system_ext/etc/permissions/privapp-permissions-google-system_ext.xml",
    ] {
        ensure!(
            selected.iter().any(|(_, p)| p == required),
            "Missing required GApps file: {required}"
        );
    }
    selected.sort_by_key(|(_, p)| (!p.contains("/etc/"), p.clone()));
    let expected: Vec<String> =
        serde_json::from_str(include_str!("../../../../engine/google-apps-files.json"))?;
    ensure!(
        selected.iter().map(|(_, p)| p).eq(expected.iter()),
        "Pinned Google services file list changed"
    );
    Ok(selected)
}
pub(super) fn prepare(
    store: &Store,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<Payload> {
    let lock = source();
    let cache = store.root().join("google-apps-cache");
    fs::create_dir_all(&cache)?;
    ensure!(
        fs2::available_space(&cache)? > 2 * 1024 * 1024 * 1024,
        "Free at least 2 GiB for the Google services download"
    );
    let archive = cache.join("package.zip");
    if archive.exists()
        && (archive.metadata()?.len() != lock.bytes || digest(&archive)? != lock.sha256)
    {
        fs::remove_file(&archive)?;
    }
    if !archive.exists() {
        let partial = cache.join("package.partial");
        if partial.exists() {
            fs::remove_file(&partial)?;
        }
        let result =
            glidepod_macos::download::file(&lock.url, &partial, lock.bytes, &mut |received| {
                progress(Progress {
                    message: "Downloading Google services…".into(),
                    completed: received,
                    total: lock.bytes,
                });
                !cancel.load(Ordering::Relaxed)
            });
        if let Err(e) = result {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
        if partial.metadata()?.len() != lock.bytes || digest(&partial)? != lock.sha256 {
            fs::remove_file(&partial)?;
            bail!("Google services download failed verification");
        }
        fs::rename(partial, &archive)?;
    }
    check_cancel(cancel)?;
    emit(progress, "Verifying and preparing Google services…");
    ensure!(digest(&archive)? == lock.sha256, "Cached download changed");
    let mut zip = zip::ZipArchive::new(File::open(&archive)?)?;
    let selected = entries(&mut zip)?;
    let directory = cache.join("payload");
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    fs::create_dir(&directory)?;
    let mut files = Vec::new();
    let mut bytes = 0;
    for (index, path) in selected {
        check_cancel(cancel)?;
        let mut input = zip.by_index(index)?;
        let target = directory.join(&path);
        fs::create_dir_all(target.parent().unwrap())?;
        let mut output = File::create(target)?;
        bytes += std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        files.push(path);
    }
    Ok(Payload {
        directory,
        files,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    fn archive(extra: Option<&str>, sdk: &str) -> Result<zip::ZipArchive<Cursor<Vec<u8>>>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("build.prop", options)?;
        write!(zip, "arch=arm64\nversion={sdk}\n")?;
        let paths: Vec<String> =
            serde_json::from_str(include_str!("../../../../engine/google-apps-files.json"))?;
        for path in paths {
            zip.start_file(format!("system/{path}"), options)?;
            zip.write_all(b"fixture")?;
        }
        if let Some(path) = extra {
            zip.start_file(path, options)?;
            zip.write_all(b"bad")?;
        }
        Ok(zip::ZipArchive::new(zip.finish()?)?)
    }
    #[test]
    fn archives_require_the_pinned_layout_and_reject_unsafe_or_conflicting_paths() -> Result<()> {
        let selected = entries(&mut archive(None, "37")?)?;
        assert!(selected.first().unwrap().1.contains("/etc/"));
        assert!(selected.iter().any(|(_, p)| p.ends_with("prodvic.apex")));
        for path in [
            "system/product/../escape",
            "system/product/app/bad;command.apk",
            "BUILD.PROP",
            "system/product/lib/extra.so",
        ] {
            assert!(entries(&mut archive(Some(path), "37")?).is_err(), "{path}");
        }
        assert!(entries(&mut archive(None, "36")?).is_err());
        Ok(())
    }
}
