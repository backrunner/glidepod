#!/usr/bin/env python3
"""Compile and execute the pinned candidate's VMM tests without linking it into GlidePod."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def run(*args, **kwargs):
    return subprocess.run([*map(str, args)], cwd=ROOT, check=True, **kwargs)

def main():
    lock = json.loads((ROOT / "engine/upstream-lock.json").read_text())
    checkout = ROOT / ".cache/upstream/avo"
    if not checkout.exists():
        checkout.parent.mkdir(parents=True, exist_ok=True)
        run("git", "clone", "--filter=blob:none", lock["repository"], checkout)
        run("git", "-C", checkout, "checkout", "--detach", lock["revision"])
    head = run("git", "-C", checkout, "rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    dirty = run("git", "-C", checkout, "status", "--porcelain", capture_output=True, text=True).stdout.strip()
    if head != lock["revision"] or dirty:
        raise SystemExit("The evaluation checkout must match the lock and contain no tracked or untracked changes. Preserve your work before replacing it.")
    build = run("cargo", "test", "--locked", "--manifest-path", checkout / "engine/hvf/Cargo.toml", "-p", "avo-vmm", "--no-run", "--message-format=json", capture_output=True, text=True)
    binaries = []
    for line in build.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("profile", {}).get("test") and message.get("executable"):
            binaries.append(message["executable"])
    if not binaries:
        raise SystemExit("Cargo did not produce a test executable.")
    logs = []
    for binary in binaries:
        run("codesign", "--force", "--sign", "-", "--entitlements", ROOT / "macos/packaging/hypervisor.entitlements", binary)
        result = run(binary, "--test-threads=1", capture_output=True, text=True)
        logs.append(result.stdout + result.stderr)
    output = ROOT / "evidence/g0/upstream-vmm-tests.log"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(logs))
    print(output)

if __name__ == "__main__":
    main()
