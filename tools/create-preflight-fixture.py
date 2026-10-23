#!/usr/bin/env python3
"""Generate an unsigned, non-executable APK-shaped archive for metadata tests only."""
from pathlib import Path
import struct
import zipfile

ROOT = Path(__file__).resolve().parents[1]
NONE = 0xFFFFFFFF
STRINGS = ["android", "http://schemas.android.com/apk/res/android", "manifest", "package", "dev.glidepod.preflight", "versionName", "0.1-test", "versionCode", "uses-sdk", "minSdkVersion", "targetSdkVersion", "application", "label", "GlidePod 测试"]

def header(kind, size, body):
    return struct.pack("<HHI", kind, size, 8 + len(body)) + body

def string_pool():
    content = bytearray()
    offsets = []
    for value in STRINGS:
        offsets.append(len(content))
        encoded = value.encode("utf-8")
        units = len(value.encode("utf-16-le")) // 2
        assert units < 128 and len(encoded) < 128
        content.extend(bytes([units, len(encoded)]) + encoded + b"\0")
    while len(content) % 4:
        content.append(0)
    body = struct.pack("<IIIII", len(STRINGS), 0, 0x100, 28 + len(STRINGS) * 4, 0)
    body += b"".join(struct.pack("<I", n) for n in offsets) + content
    return header(1, 28, body)

def node(kind, payload):
    return header(kind, 16, struct.pack("<II", 1, NONE) + payload)

def attribute(name, value, namespace=True):
    integer = isinstance(value, int)
    return struct.pack("<IIIHBBI", 1 if namespace else NONE, STRINGS.index(name), NONE if integer else STRINGS.index(value), 8, 0, 0x10 if integer else 3, value if integer else STRINGS.index(value))

def start(name, attributes):
    return node(0x102, struct.pack("<IIHHHHHH", NONE, STRINGS.index(name), 20, 20, len(attributes), 0, 0, 0) + b"".join(attributes))

def end(name):
    return node(0x103, struct.pack("<II", NONE, STRINGS.index(name)))

def main():
    body = string_pool() + node(0x100, struct.pack("<II", 0, 1))
    body += start("manifest", [attribute("package", "dev.glidepod.preflight", False), attribute("versionName", "0.1-test"), attribute("versionCode", 1)])
    body += start("uses-sdk", [attribute("minSdkVersion", 29), attribute("targetSdkVersion", 35)]) + end("uses-sdk")
    body += start("application", [attribute("label", "GlidePod 测试")]) + end("application")
    body += end("manifest") + node(0x101, struct.pack("<II", 0, 1))
    manifest = header(3, 8, body)
    fixture = ROOT / "crates/glidepod-installer/tests/fixtures/manifest.axml"
    fixture.parent.mkdir(parents=True, exist_ok=True)
    fixture.write_bytes(manifest)
    apk = ROOT / ".cache/fixtures/GlidePod-Preflight.apk"
    apk.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(apk, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, data in [("AndroidManifest.xml", manifest), ("lib/arm64-v8a/libfixture.so", b"Metadata fixture only; not executable ELF.")]:
            entry = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(entry, data)
    print(apk)

if __name__ == "__main__":
    main()
