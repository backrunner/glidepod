#!/usr/bin/env python3
"""Register the pinned external evaluation build with the local management center."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
def run(*args):
    return subprocess.run([*map(str, args)], cwd=ROOT, capture_output=True, text=True, check=True)
def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-dir", type=Path)
    args = parser.parse_args()
    upstream = ROOT / ".cache/upstream/avo"
    lock = json.loads((ROOT / "engine/upstream-lock.json").read_text())
    build = json.loads((ROOT / "engine/cuttlefish-lock.json").read_text())["build_id"]
    assert run("git", "-C", upstream, "rev-parse", "HEAD").stdout.strip() == lock["revision"]
    assert not run("git", "-C", upstream, "status", "--porcelain").stdout.strip()
    report = json.loads((ROOT / "evidence/g0/android-boot/report.json").read_text())
    assert report["android_boot_completed"] and report["build_id"] == build
    # The registration is a measured local inventory, not a publisher signature.
    fingerprint = "generic/aosp_cf_arm64_only_phone/vsoc_arm64_only:17/CP2A.260605.016/16644040:userdebug/test-keys"
    cli = ROOT / "target/debug/glidepodctl"
    command = [cli]
    if args.data_dir: command += ["--data-dir", args.data_dir]
    command += ["register-experimental", "--engine", upstream / "engine/hvf/target/release/avo-vmm", "--images", ROOT / ".cache/cuttlefish" / build / "images", "--fingerprint", fingerprint]
    print(run(*command).stdout.strip())
if __name__ == "__main__": main()
