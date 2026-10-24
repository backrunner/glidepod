#!/usr/bin/env python3
"""Build a local ad-hoc signed development app; never notarize, publish or install globally."""
import argparse
import plistlib
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    subprocess.run([*map(str, args)], cwd=ROOT, check=True)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if not args.skip_build:
        run("python3", "tools/build-android.py")
        run("cargo", "build", "--workspace", "--locked")
    bundle = ROOT / "dist/GlidePod.app"
    macos = bundle / "Contents/MacOS"
    resources = bundle / "Contents/Resources"
    macos.mkdir(parents=True, exist_ok=True)
    resources.mkdir(parents=True, exist_ok=True)
    for binary in ("glidepod", "glidepodctl"):
        shutil.copy2(ROOT / "target/debug" / binary, macos / binary)
    run("xcrun", "swiftc", "-O", ROOT / "macos/packaging/AppLauncher.swift", "-o", macos / "glidepod-launcher")
    run("codesign", "--force", "--sign", "-", macos / "glidepod-launcher")
    shutil.copy2(macos / "glidepod-launcher", ROOT / "target/debug/glidepod-launcher")
    (resources / "android").mkdir(exist_ok=True)
    for name in ("glidepod-bridge.jar", "glidepod-widgets.apk", "glidepod-input.apk", "glidepod-notifications.apk", "glidepod-benchmark.apk", "glidepod-vulkan.apk"):
        shutil.copy2(ROOT / "target/android" / name, resources / "android" / name)
    icon_work = ROOT / ".cache/icon-compiled"
    icon_work.mkdir(parents=True, exist_ok=True)
    # Preserve Apple's layered icon and appearance renditions in Assets.car.
    # actool emits only a partial ICNS. Build the full 1x/2x fallback from the
    # catalog's native renditions, including 512pt @2x (1024 physical pixels).
    run("xcrun", "actool", ROOT / "assets/GlidePod.icon", "--compile", icon_work,
        "--platform", "macosx", "--minimum-deployment-target", "15.0",
        "--app-icon", "GlidePod", "--output-partial-info-plist", icon_work / "partial.plist",
        "--output-format", "human-readable-text", "--notices", "--warnings")
    icon_bundle = icon_work / "GlidePodIcons.bundle"
    icon_resources = icon_bundle / "Contents/Resources"
    icon_resources.mkdir(parents=True, exist_ok=True)
    shutil.copy2(icon_work / "Assets.car", icon_resources / "Assets.car")
    with (icon_bundle / "Contents/Info.plist").open("wb") as file:
        plistlib.dump({"CFBundleIdentifier": "dev.glidepod.icon-export", "CFBundlePackageType": "BNDL"}, file)
    icon_exporter = icon_work / "export-app-icon"
    run("xcrun", "swiftc", ROOT / "macos/packaging/ExportAppIcon.swift", "-o", icon_exporter)
    iconset = icon_work / "GlidePod.iconset"
    run(icon_exporter, icon_bundle, iconset)
    run("iconutil", "-c", "icns", iconset, "-o", icon_work / "GlidePod.icns")
    for name in ("Assets.car", "GlidePod.icns"):
        shutil.copy2(icon_work / name, resources / name)
    shutil.copy2(ROOT / "LICENSE", resources / "LICENSE")
    for notice in ("NOTICE", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSES.txt"):
        if (ROOT / notice).exists():
            shutil.copy2(ROOT / notice, resources / notice)
    info = {
        "CFBundleName": "GlidePod", "CFBundleDisplayName": "GlidePod",
        "CFBundleIdentifier": "dev.glidepod.desktop", "CFBundleExecutable": "glidepod",
        "CFBundleVersion": "1", "CFBundleShortVersionString": "0.1.0",
        "CFBundlePackageType": "APPL", "CFBundleIconFile": "GlidePod",
        "LSMinimumSystemVersion": "15.0", "NSHighResolutionCapable": True,
        "CFBundleURLTypes": [{"CFBundleURLName": "GlidePod app", "CFBundleURLSchemes": ["glidepod"]}],
        "CFBundleDocumentTypes": [{"CFBundleTypeName": "Android application package", "CFBundleTypeRole": "Viewer", "LSHandlerRank": "Alternate", "LSItemContentTypes": ["org.android.package-archive"]}],
        "UTImportedTypeDeclarations": [{"UTTypeIdentifier": "org.android.package-archive", "UTTypeDescription": "Android application package", "UTTypeConformsTo": ["public.zip-archive"], "UTTypeTagSpecification": {"public.filename-extension": ["apk"], "public.mime-type": ["application/vnd.android.package-archive"]}}],
    }
    with (icon_work / "partial.plist").open("rb") as file:
        info.update(plistlib.load(file))
    with (bundle / "Contents/Info.plist").open("wb") as file:
        plistlib.dump(info, file)
    run("codesign", "--force", "--sign", "-", "--entitlements", ROOT / "macos/packaging/hypervisor.entitlements", macos / "glidepodctl")
    run("codesign", "--force", "--sign", "-", bundle)
    run("codesign", "--verify", "--deep", "--strict", bundle)
    # Contents changes alone do not update the app directory's modification time.
    # Let Launch Services see a rebuilt development bundle and its new icon.
    bundle.touch()
    print(bundle)

if __name__ == "__main__":
    main()
