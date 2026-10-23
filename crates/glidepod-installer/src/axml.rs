//! Bounded Android binary XML reader for manifest metadata, never a signature verifier.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
const ANDROID: &str = "http://schemas.android.com/apk/res/android";
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub package_id: String,
    pub version_name: Option<String>,
    pub version_code: Option<u32>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    pub label: Option<String>,
}
fn bytes(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    data.get(
        offset
            ..offset
                .checked_add(len)
                .ok_or_else(|| anyhow::anyhow!("XML offset overflow"))?,
    )
    .ok_or_else(|| anyhow::anyhow!("Truncated binary XML"))
}
fn u16_at(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(data, offset, 2)?.try_into()?))
}
fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, offset, 4)?.try_into()?))
}
fn length8(data: &[u8], at: &mut usize) -> Result<usize> {
    let first = bytes(data, *at, 1)?[0];
    *at += 1;
    if first & 0x80 == 0 {
        Ok(first as usize)
    } else {
        let next = bytes(data, *at, 1)?[0];
        *at += 1;
        Ok((((first & 0x7f) as usize) << 8) | next as usize)
    }
}
fn length16(data: &[u8], at: &mut usize) -> Result<usize> {
    let first = u16_at(data, *at)?;
    *at += 2;
    if first & 0x8000 == 0 {
        Ok(first as usize)
    } else {
        let next = u16_at(data, *at)?;
        *at += 2;
        Ok((((first & 0x7fff) as usize) << 16) | next as usize)
    }
}
fn pool(data: &[u8], header: usize) -> Result<Vec<String>> {
    ensure!(header >= 28, "Invalid string-pool header");
    let count = u32_at(data, 8)? as usize;
    let styles = u32_at(data, 12)? as usize;
    ensure!(
        count <= 65536 && styles <= 65536,
        "String pool is too large"
    );
    let utf8 = u32_at(data, 16)? & 0x100 != 0;
    let start = u32_at(data, 20)? as usize;
    let style_start = u32_at(data, 24)? as usize;
    ensure!(
        start >= header + (count + styles) * 4 && start <= data.len(),
        "Invalid string-pool offsets"
    );
    let end = if style_start == 0 {
        data.len()
    } else {
        ensure!(
            style_start >= start && style_start <= data.len(),
            "Invalid style offset"
        );
        style_start
    };
    let content = bytes(data, start, end - start)?;
    let mut decoded_bytes = 0usize;
    (0..count)
        .map(|i| {
            let mut at = u32_at(data, header + i * 4)? as usize;
            if utf8 {
                let units = length8(content, &mut at)?;
                let len = length8(content, &mut at)?;
                decoded_bytes += len;
                ensure!(
                    decoded_bytes <= 8 * 1024 * 1024,
                    "Decoded string pool exceeds 8 MiB"
                );
                let value = std::str::from_utf8(bytes(content, at, len)?)?;
                ensure!(
                    bytes(content, at + len, 1)?[0] == 0 && value.encode_utf16().count() == units,
                    "Invalid UTF-8 string-pool entry"
                );
                Ok(value.to_string())
            } else {
                let len = length16(content, &mut at)?;
                ensure!(len <= 2 * 1024 * 1024, "String is too large");
                decoded_bytes += len * 3;
                ensure!(
                    decoded_bytes <= 8 * 1024 * 1024,
                    "Decoded string pool exceeds 8 MiB"
                );
                let words = bytes(content, at, len * 2)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|v| u16::from_le_bytes([v[0], v[1]]))
                    .collect::<Vec<_>>();
                ensure!(
                    u16_at(content, at + len * 2)? == 0,
                    "Missing UTF-16 terminator"
                );
                Ok(String::from_utf16(&words)?)
            }
        })
        .collect()
}
fn string(strings: &[String], index: u32) -> Result<&str> {
    strings
        .get(index as usize)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid string-pool reference"))
}
pub fn parse(data: &[u8]) -> Result<Manifest> {
    ensure!(data.len() <= 4 * 1024 * 1024, "Manifest exceeds 4 MiB");
    ensure!(
        u16_at(data, 0)? == 3 && u16_at(data, 2)? == 8 && u32_at(data, 4)? as usize == data.len(),
        "Expected a complete Android binary XML manifest"
    );
    let mut at = 8;
    let mut strings = None;
    let mut manifest = Manifest::default();
    let mut found = false;
    let mut elements = Vec::new();
    while at < data.len() {
        let kind = u16_at(data, at)?;
        let header = u16_at(data, at + 2)? as usize;
        let len = u32_at(data, at + 4)? as usize;
        ensure!(header >= 8 && len >= header, "Invalid XML chunk length");
        let chunk = bytes(data, at, len)?;
        if kind == 1 {
            ensure!(strings.is_none(), "Multiple string pools");
            strings = Some(pool(chunk, header)?);
        }
        if kind == 0x102 {
            ensure!(header == 16 && len >= 36, "Invalid start-element header");
            let strings = strings
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing string pool"))?;
            let tag = string(strings, u32_at(chunk, 20)?)?;
            let namespace = u32_at(chunk, 16)?;
            ensure!(elements.len() < 128, "XML nesting exceeds 128 elements");
            if elements.is_empty() {
                ensure!(
                    tag == "manifest" && namespace == u32::MAX && !found,
                    "Expected one manifest root"
                );
            }
            if tag == "manifest" {
                ensure!(
                    !found && elements.is_empty(),
                    "Duplicate or nested manifest element"
                );
                found = true;
            }
            let metadata_tag =
                namespace == u32::MAX && (elements.is_empty() || elements.len() == 1);
            elements.push((namespace, u32_at(chunk, 20)?));
            let offset = u16_at(chunk, 24)? as usize;
            let stride = u16_at(chunk, 26)? as usize;
            let count = u16_at(chunk, 28)? as usize;
            ensure!(
                offset >= 20 && stride >= 20 && count <= 4096,
                "Invalid attribute table"
            );
            bytes(chunk, 16 + offset, stride * count)?;
            let mut attributes = std::collections::BTreeSet::new();
            for i in 0..count {
                let attr = bytes(chunk, 16 + offset + i * stride, stride)?;
                let ns = u32_at(attr, 0)?;
                let name = string(strings, u32_at(attr, 4)?)?;
                ensure!(attributes.insert((ns, name)), "Duplicate XML attribute");
                let raw = u32_at(attr, 8)?;
                ensure!(u16_at(attr, 12)? == 8, "Invalid attribute value");
                let data_type = attr[15];
                let value = u32_at(attr, 16)?;
                let text = if raw != u32::MAX {
                    Some(string(strings, raw)?.to_owned())
                } else if data_type == 3 {
                    Some(string(strings, value)?.to_owned())
                } else {
                    None
                };
                let number = if matches!(data_type, 0x10 | 0x11) {
                    Some(value)
                } else {
                    text.as_ref().and_then(|v| v.parse().ok())
                };
                if !metadata_tag {
                    continue;
                }
                if tag == "manifest" && name == "package" && ns == u32::MAX {
                    manifest.package_id = text.unwrap_or_default();
                    continue;
                }
                if ns == u32::MAX || string(strings, ns)? != ANDROID {
                    continue;
                }
                match (tag, name) {
                    ("manifest", "versionName") => manifest.version_name = text,
                    ("manifest", "versionCode") => manifest.version_code = number,
                    ("uses-sdk", "minSdkVersion") => manifest.min_sdk = number,
                    ("uses-sdk", "targetSdkVersion") => manifest.target_sdk = number,
                    ("application", "label") => manifest.label = text,
                    _ => {}
                }
            }
        }
        if kind == 0x103 {
            ensure!(header == 16 && len >= 24, "Invalid end-element header");
            ensure!(
                elements.pop() == Some((u32_at(chunk, 16)?, u32_at(chunk, 20)?)),
                "Unbalanced XML elements"
            );
        }
        at += len;
    }
    ensure!(
        found && elements.is_empty(),
        "Missing or unclosed manifest element"
    );
    let parts = manifest.package_id.split('.').collect::<Vec<_>>();
    if parts.len() < 2
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    {
        bail!("Invalid Android package ID");
    }
    Ok(manifest)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_xml_is_rejected_without_panicking() {
        for n in 0..128 {
            assert!(parse(&vec![0xff; n]).is_err());
        }
        let mut data = vec![3, 0, 8, 0, 16, 0, 0, 0, 2, 1, 16, 0, 0xff, 0xff, 0xff, 0xff];
        assert!(parse(&data).is_err());
        data[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse(&data).is_err());
    }
    #[test]
    fn utf8_pool_checks_length_and_reference() {
        let mut data = vec![0u8; 32];
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        data[16..20].copy_from_slice(&0x100u32.to_le_bytes());
        data[20..24].copy_from_slice(&32u32.to_le_bytes());
        data.extend_from_slice(&[1, 1, b'x', 0]);
        assert_eq!(pool(&data, 28).unwrap(), vec!["x"]);
        data[32] = 2;
        assert!(pool(&data, 28).is_err());
    }
    #[test]
    fn repeated_string_offsets_cannot_amplify_memory_without_bound() {
        let count = 1024u32;
        let start = 28 + count * 4;
        let mut data = vec![0u8; start as usize];
        data[8..12].copy_from_slice(&count.to_le_bytes());
        data[16..20].copy_from_slice(&0x100u32.to_le_bytes());
        data[20..24].copy_from_slice(&start.to_le_bytes());
        data.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        data.extend(std::iter::repeat_n(b'x', 32767));
        data.push(0);
        assert!(
            pool(&data, 28)
                .unwrap_err()
                .to_string()
                .contains("Decoded string pool")
        );
    }
    #[test]
    fn incomplete_manifest_tree_is_rejected() {
        let mut data = include_bytes!("../tests/fixtures/manifest.axml").to_vec();
        data.truncate(data.len() - 48);
        let len = data.len() as u32;
        data[4..8].copy_from_slice(&len.to_le_bytes());
        assert!(parse(&data).unwrap_err().to_string().contains("unclosed"));
    }
}
