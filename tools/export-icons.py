#!/usr/bin/env python3
"""Export review images with Apple's macOS 27 Icon Composer renderer."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    return subprocess.run([*map(str, args)], cwd=ROOT, check=True, capture_output=True, text=True)

def main():
    developer = Path(run("xcode-select", "-p").stdout.strip())
    tool = developer.parent / "Applications/Icon Composer.app/Contents/Executables/ictool"
    if not tool.is_file():
        raise SystemExit("Select Xcode 27 with Icon Composer before exporting icons.")
    output = ROOT / "evidence/ui-refinement"
    output.mkdir(parents=True, exist_ok=True)
    variants = {"Default": "default", "Dark": "dark", "ClearLight": "clear-light", "ClearDark": "clear-dark", "TintedLight": "tinted-light", "TintedDark": "tinted-dark"}
    for rendition, name in variants.items():
        run(tool, ROOT / "assets/GlidePod.icon", "--export-image", "--output-file", output / f"icon-{name}.png", "--platform", "macOS", "--rendition", rendition, "--width", "512", "--height", "512", "--scale", "1", "--design-generation", "27")
    for size in (16, 32, 64, 128):
        run(tool, ROOT / "assets/GlidePod.icon", "--export-image", "--output-file", output / f"icon-default-{size}.png", "--platform", "macOS", "--rendition", "Default", "--width", size, "--height", size, "--scale", "1", "--design-generation", "27")
    (output / "icon-toolchain.txt").write_text(run("xcodebuild", "-version").stdout + run(tool, "--version").stdout + "Design generation: 27\n")
    print(output)

if __name__ == "__main__":
    main()
