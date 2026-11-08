#!/usr/bin/env python3
"""Test the real menu-bar adapter in-process, without desktop automation."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="glidepod-tray-native-") as directory:
    binary = Path(directory) / "tray-native"
    subprocess.run([
        "xcrun", "clang", "-fobjc-arc", "-Wall", "-Werror", "-mmacosx-version-min=15.0",
        str(ROOT / "crates/glidepod-macos/tests/tray_native.m"),
        "-framework", "AppKit", "-o", str(binary),
    ], check=True)
    subprocess.run([str(binary)], check=True, timeout=15)
