#!/usr/bin/env python3
"""Record declared dependency licenses and available upstream notice texts.

This is an inventory, not a determination that a binary is ready for distribution.
"""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def main():
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    )
    metadata = json.loads(result.stdout)
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    packages = sorted(
        (p for p in metadata["packages"] if p["id"] in resolved and p["source"]),
        key=lambda p: (p["name"], p["version"]),
    )
    records = []
    texts = ["GlidePod dependency license and notice texts\nGenerated from Cargo.lock; includes build and development dependencies.\n"]
    for package in packages:
        directory = Path(package["manifest_path"]).parent
        paths = {p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))}
        if package["license_file"]:
            paths.add(directory / package["license_file"])
        record = {key: package[key] for key in ("name", "version", "license", "source", "repository")}
        record["notice_files"] = []
        for path in sorted(paths):
            if not path.is_file() or not path.resolve().is_relative_to(directory.resolve()):
                continue
            record["notice_files"].append(str(path.relative_to(directory)))
            texts.append(f"\n{'=' * 72}\n{package['name']} {package['version']} — {path.name}\n{'=' * 72}\n{path.read_text(errors='replace')}\n")
        records.append(record)
    output = ROOT / "evidence/g0"
    output.mkdir(parents=True, exist_ok=True)
    (output / "dependency-licenses.json").write_text(json.dumps({"target": "aarch64-apple-darwin", "scope": "Cargo resolved graph, including build and dev dependencies", "packages": records}, indent=2) + "\n")
    public_output = ROOT / "licenses"
    public_output.mkdir(exist_ok=True)
    (public_output / "dependency-inventory.json").write_text((output / "dependency-licenses.json").read_text())
    (ROOT / "THIRD_PARTY_LICENSES.txt").write_text("".join(texts))
    missing = [f"{p['name']}@{p['version']}" for p in records if not p["notice_files"]]
    print(json.dumps({"packages": len(records), "missing_notice_texts": missing}))

if __name__ == "__main__":
    main()
