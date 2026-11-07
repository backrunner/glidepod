#!/usr/bin/env python3
"""Exercise the real AppKit file adapter against a private, isolated pasteboard."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="glidepod-files-native-") as directory:
    binary = Path(directory) / "files-native"
    subprocess.run([
        "xcrun", "clang", "-fobjc-arc", "-Wall", "-Werror", "-mmacosx-version-min=15.0",
        str(ROOT / "crates/glidepod-macos/tests/files_native.m"),
        "-framework", "AppKit", "-framework", "UniformTypeIdentifiers", "-framework", "ImageIO",
        "-o", str(binary),
    ], check=True)
    subprocess.run([str(binary)], check=True)
