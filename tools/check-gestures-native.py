#!/usr/bin/env python3
"""Check the AppKit adapter with in-process event objects; never post desktop input."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="glidepod-gestures-native-") as directory:
    binary = Path(directory) / "gestures-native"
    subprocess.run([
        "xcrun", "clang", "-fobjc-arc", "-Wall", "-Werror", "-mmacosx-version-min=15.0",
        str(ROOT / "crates/glidepod-macos/tests/gestures_native.m"),
        "-framework", "AppKit", "-o", str(binary),
    ], check=True)
    subprocess.run([str(binary)], check=True)
