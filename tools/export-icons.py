#!/usr/bin/env python3
"""Export review and interface images from the editable Icon Composer master."""
from pathlib import Path
import subprocess
import shutil

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    return subprocess.run([*map(str, args)], cwd=ROOT, check=True, capture_output=True, text=True)

def main():
    developer = Path(run("xcode-select", "-p").stdout.strip())
    tool = developer.parent / "Applications/Icon Composer.app/Contents/Executables/ictool"
    if not tool.is_file():
        raise SystemExit("Select Xcode 27 with Icon Composer before exporting icons.")
    # Generate the soft radial background separately. Package faces stay vector.
    cache = ROOT / ".cache"
    cache.mkdir(exist_ok=True)
    generator = cache / "generate-icon-background"
    run("xcrun", "swiftc", "-O", ROOT / "tools/generate-icon-background.swift", "-o", generator)
    background = ROOT / "assets/GlidePod.icon/Assets/00-gradient-background.png"
    run(generator, ROOT / "assets/icon-background.json", background)
    shutil.copy2(background, ROOT / ".agents/design/icon-composer/GlidePod.icon/Assets" / background.name)
    output = ROOT / "evidence/ui-refinement"
    output.mkdir(parents=True, exist_ok=True)
    variants = {"Default": "default", "Dark": "dark", "ClearLight": "clear-light", "ClearDark": "clear-dark", "TintedLight": "tinted-light", "TintedDark": "tinted-dark"}
    for rendition, name in variants.items():
        run(tool, ROOT / "assets/GlidePod.icon", "--export-image", "--output-file", output / f"icon-{name}.png", "--platform", "macOS", "--rendition", rendition, "--width", "512", "--height", "512", "--scale", "1", "--design-generation", "27")
        if name in ("default", "dark"):
            shutil.copy2(output / f"icon-{name}.png", ROOT / f".agents/design/icon-composer/preview-{name}.png")
    for size in (16, 32, 64, 128):
        run(tool, ROOT / "assets/GlidePod.icon", "--export-image", "--output-file", output / f"icon-default-{size}.png", "--platform", "macOS", "--rendition", "Default", "--width", size, "--height", size, "--scale", "1", "--design-generation", "27")
    shutil.copy2(output / "icon-default-32.png", ROOT / ".agents/design/icon-composer/preview-default-32.png")
    # Interface images share the app icon master. Keep the prototype self-contained.
    # The macOS bundle still uses the layered Assets.car and ICNS from actool.
    brand = ROOT / "assets/brand"
    prototype = ROOT / ".agents/prototype/assets"
    brand.mkdir(parents=True, exist_ok=True)
    for rendition, name in (("Default", "light"), ("Dark", "dark")):
        path = brand / f"glidepod-icon-{name}.png"
        run(tool, ROOT / "assets/GlidePod.icon", "--export-image", "--output-file", path,
            "--platform", "macOS", "--rendition", rendition, "--width", "256", "--height", "256",
            "--scale", "1", "--design-generation", "27")
        shutil.copy2(path, prototype / path.name)
        # GPUI's atlas uses bilinear sampling without mipmaps. Render the sidebar
        # at its physical pixel size instead of shrinking the 256px preview.
        for scale in (1, 2, 3):
            path = brand / f"glidepod-sidebar-{name}@{scale}x.png"
            run(tool, ROOT / "assets/GlidePod.icon", "--export-image", "--output-file", path,
                "--platform", "macOS", "--rendition", rendition, "--width", "40", "--height", "40",
                "--scale", scale, "--design-generation", "27")
            shutil.copy2(path, prototype / path.name)
    (output / "icon-toolchain.txt").write_text(run("xcodebuild", "-version").stdout + run(tool, "--version").stdout + "Design generation: 27\n")
    print(output)

if __name__ == "__main__":
    main()
