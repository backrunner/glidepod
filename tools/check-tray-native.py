#!/usr/bin/env python3
"""Exercise the SwiftUI/AppKit tray and its C ABI in-process.

Pass --render-dir to export the actual native view in both appearances. These
fixture renders and native callbacks do not establish physical desktop clicks.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--render-dir", type=Path)
parser.add_argument("--preview", action="store_true", help="Build an isolated native fixture app under .cache")
parser.add_argument("--icons-dir", type=Path, help="Optional existing guest icon cache for fixture review")
args = parser.parse_args()
if args.preview:
    bundle = ROOT / ".cache/tray-preview/GlidePod Tray Preview.app"
    binary = bundle / "Contents/MacOS/tray-preview"
    resources = bundle / "Contents/Resources"
    binary.parent.mkdir(parents=True, exist_ok=True)
    resources.mkdir(parents=True, exist_ok=True)
    subprocess.run([
        "xcrun", "--sdk", "macosx", "swiftc", "-swift-version", "6", "-warnings-as-errors",
        "-parse-as-library", "-target", f"{platform.machine()}-apple-macosx15.0",
        *map(str, sorted((ROOT / "crates/glidepod-macos/src/tray").glob("*.swift"))),
        str(ROOT / "crates/glidepod-macos/tests/tray_preview.swift"), "-o", str(binary),
    ], check=True)
    shutil.copy2(ROOT / "assets/brand/glidepod-icon-light.png", resources / "GlidePod.png")
    apps = []
    for package, title in [
        ("com.android.deskclock", "时钟"), ("com.android.settings", "Settings"),
        ("com.android.calendar", "Calendar"), ("com.android.documentsui", "Files"),
        ("com.android.camera2", "Camera"), ("com.android.contacts", "Contacts"),
        ("dev.glidepod.long", "一个很长的应用名称 · A deliberately long application name"),
        ("com.android.messaging", "Messages"), ("com.android.gallery3d", "Gallery"),
    ]:
        icon = args.icons_dir / f"{package}.png" if args.icons_dir else None
        if icon and icon.is_file():
            fixture_icon = resources / f"{package}.png"
            shutil.copy2(icon, fixture_icon)
            icon = fixture_icon
        apps.append({"package_id": package, "title": title,
                     "icon_path": str(icon.resolve()) if icon and icon.is_file() else None})
    (resources / "fixture.json").write_text(json.dumps({
        "state": "Running", "detail": "Fixture data", "cpu": "142.5%", "memory": "2.50 GiB",
        "allocation": "4 vCPUs · 4.00 GiB", "app_count": len(apps), "apps": apps,
        "valid_for_seconds": 15,
    }))
    with (bundle / "Contents/Info.plist").open("wb") as file:
        plistlib.dump({"CFBundleIdentifier": "dev.glidepod.tray-preview", "CFBundleName": "GlidePod Tray Preview",
                      "CFBundleExecutable": "tray-preview", "CFBundlePackageType": "APPL",
                      "CFBundleIconName": "GlidePod", "NSHighResolutionCapable": True}, file)
    subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], check=True)
    print(bundle)
    raise SystemExit(0)
with tempfile.TemporaryDirectory(prefix="glidepod-tray-native-") as directory:
    binary = Path(directory) / "tray-native"
    subprocess.run([
        "xcrun", "--sdk", "macosx", "swiftc", "-swift-version", "6", "-warnings-as-errors",
        "-parse-as-library", "-target", f"{platform.machine()}-apple-macosx15.0",
        *map(str, sorted((ROOT / "crates/glidepod-macos/src/tray").glob("*.swift"))),
        str(ROOT / "crates/glidepod-macos/tests/tray_native.swift"), "-o", str(binary),
    ], check=True)
    command = [str(binary), str(ROOT / "assets/brand/glidepod-icon-light.png")]
    if args.render_dir:
        command.append(str(args.render_dir.resolve()))
    environment = dict(os.environ)
    if args.icons_dir:
        environment["GLIDEPOD_TRAY_FIXTURE_ICONS"] = str(args.icons_dir.resolve())
    subprocess.run(command, check=True, timeout=20, env=environment)
