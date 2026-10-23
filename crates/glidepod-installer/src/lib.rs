//! Local APK preflight with bounded archive reads. Installation requires a qualified guest backend.
mod axml;
use anyhow::{Context, Result, ensure};
pub use axml::Manifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Component, Path},
};
const MAX_APK: u64 = 2 * 1024 * 1024 * 1024;
/// Bound central-directory allocation before handing untrusted bytes to the ZIP library.
fn check_directory(file: &mut File, size: u64) -> Result<()> {
    let tail_size = size.min(65535 + 22) as usize;
    file.seek(SeekFrom::End(-(tail_size as i64)))?;
    let mut tail = vec![0; tail_size];
    file.read_exact(&mut tail)?;
    let at = tail
        .windows(4)
        .enumerate()
        .rev()
        .find_map(|(i, marker)| {
            if marker != b"PK\x05\x06" || i + 22 > tail.len() {
                return None;
            }
            let comment = u16::from_le_bytes([tail[i + 20], tail[i + 21]]) as usize;
            (i + 22 + comment == tail.len()).then_some(i)
        })
        .context("Missing ZIP end-of-directory record")?;
    let word = |n| u16::from_le_bytes([tail[at + n], tail[at + n + 1]]);
    let dword = |n| u32::from_le_bytes(tail[at + n..at + n + 4].try_into().unwrap()) as u64;
    ensure!(
        word(4) == 0 && word(6) == 0 && word(8) == word(10),
        "Multi-volume APK archives are unsupported"
    );
    ensure!(
        word(10) <= 32768 && dword(12) <= 16 * 1024 * 1024,
        "APK central directory exceeds limits; ZIP64 is unsupported"
    );
    let end_record = size - tail_size as u64 + at as u64;
    ensure!(
        dword(16) + dword(12) <= end_record,
        "APK central directory is outside the archive"
    );
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApkReport {
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
    pub manifest: Manifest,
    pub native_abis: Vec<String>,
    pub arm64_candidate: bool,
    pub signature_verified: bool,
    pub warnings: Vec<String>,
}
pub fn inspect(path: &Path) -> Result<ApkReport> {
    ensure!(
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("apk")),
        "Choose an .apk file."
    );
    let mut file = File::open(path).context("Cannot open APK")?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.len() > 0 && meta.len() <= MAX_APK,
        "APK must be a regular file between 1 byte and 2 GiB."
    );
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut read = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        read += n as u64;
        ensure!(read <= MAX_APK, "APK grew beyond the size limit.");
        hash.update(&buf[..n]);
    }
    ensure!(read == meta.len(), "APK changed while it was read.");
    check_directory(&mut file, read)?;
    let mut zip = zip::ZipArchive::new(file).context("Invalid APK ZIP archive")?;
    ensure!(zip.len() <= 32768, "APK contains too many ZIP entries.");
    let mut names = BTreeSet::new();
    let mut abis = BTreeSet::new();
    let mut manifest = None;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_owned();
        ensure!(names.insert(name.clone()), "Duplicate ZIP entry: {name}");
        ensure!(
            !name.contains('\\')
                && Path::new(&name)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "Unsafe ZIP path: {name}"
        );
        ensure!(
            !entry.is_symlink(),
            "Symbolic links are not supported in APKs."
        );
        total = total
            .checked_add(entry.size())
            .context("ZIP size overflow")?;
        ensure!(
            total <= 8 * 1024 * 1024 * 1024,
            "APK expanded size exceeds 8 GiB."
        );
        ensure!(
            entry.size()
                <= entry
                    .compressed_size()
                    .max(1)
                    .saturating_mul(200)
                    .max(1024 * 1024),
            "Suspicious compression ratio: {name}"
        );
        let parts = name.split('/').collect::<Vec<_>>();
        if parts.len() == 3 && parts[0] == "lib" && parts[2].ends_with(".so") {
            abis.insert(parts[1].to_owned());
        }
        if name == "AndroidManifest.xml" {
            ensure!(entry.size() <= 4 * 1024 * 1024, "Manifest exceeds 4 MiB.");
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            manifest = Some(axml::parse(&bytes).context("Cannot parse Android manifest")?);
        }
    }
    let manifest = manifest.context("APK has no AndroidManifest.xml")?;
    let arm64_candidate = abis.is_empty() || abis.contains("arm64-v8a");
    let mut warnings=vec!["APK signing certificates have not been verified. Preflight is not approval to install.".into(),"Minimum SDK, page-size compatibility and required features must also match the selected Android image.".into()];
    if !arm64_candidate {
        warnings.push("Native libraries do not include arm64-v8a.".into());
    }
    Ok(ApkReport {
        filename: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        bytes: read,
        sha256: format!("{:x}", hash.finalize()),
        manifest,
        native_abis: abis.into_iter().collect(),
        arm64_candidate,
        signature_verified: false,
        warnings,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn archive(entries: &[(&str, &[u8])]) -> tempfile::TempPath {
        let f = tempfile::Builder::new().suffix(".apk").tempfile().unwrap();
        let mut w = zip::ZipWriter::new(f.reopen().unwrap());
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        f.into_temp_path()
    }
    #[test]
    fn parses_binary_manifest_and_reports_unverified_signature() {
        let manifest = include_bytes!("../tests/fixtures/manifest.axml");
        let path = archive(&[
            ("AndroidManifest.xml", manifest),
            ("lib/arm64-v8a/libsample.so", b"fixture"),
        ]);
        let report = inspect(&path).unwrap();
        assert_eq!(report.manifest.package_id, "dev.glidepod.preflight");
        assert_eq!(report.manifest.min_sdk, Some(29));
        assert_eq!(report.manifest.version_name.as_deref(), Some("0.1-test"));
        assert_eq!(report.manifest.label.as_deref(), Some("GlidePod 测试"));
        assert!(report.arm64_candidate);
        assert!(!report.signature_verified);
        let x86 = archive(&[
            ("AndroidManifest.xml", manifest),
            ("lib/x86/libsample.so", b"fixture"),
        ]);
        assert!(!inspect(&x86).unwrap().arm64_candidate);
    }
    #[test]
    fn traversal_is_rejected_before_extracting() {
        let p = archive(&[("../outside", b"x")]);
        assert!(
            inspect(&p)
                .unwrap_err()
                .to_string()
                .contains("Unsafe ZIP path")
        );
    }
    #[test]
    fn missing_manifest_is_not_a_valid_apk() {
        let p = archive(&[("classes.dex", b"dex")]);
        assert!(
            inspect(&p)
                .unwrap_err()
                .to_string()
                .contains("no AndroidManifest")
        );
    }
    #[test]
    fn oversized_directory_is_rejected_before_zip_allocation() {
        let path = archive(&[("classes.dex", b"dex")]);
        let mut bytes = std::fs::read(&path).unwrap();
        let end = bytes.len() - 22;
        bytes[end + 12..end + 16].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(
            inspect(&path)
                .unwrap_err()
                .to_string()
                .contains("central directory exceeds")
        );
    }
}
