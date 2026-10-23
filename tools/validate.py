#!/usr/bin/env python3
"""Run local foundation checks and preserve their evidence; never claim Android readiness."""
import datetime
import hashlib
import json
import plistlib
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "evidence/g0"

def run(name, *command, expected=0):
    result = subprocess.run([*map(str, command)], cwd=ROOT, capture_output=True, text=True)
    log = f"Command: {' '.join(map(str, command))}\nExit: {result.returncode}\n\n{result.stdout}{result.stderr}"
    (EVIDENCE / f"{name}.log").write_text(log.replace(str(ROOT), "<workspace>"))
    if result.returncode != expected:
        raise SystemExit(f"{name} failed; see evidence/g0/{name}.log")
    return result

def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    run("fmt", "cargo", "fmt", "--all", "--", "--check")
    run("clippy", "cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")
    run("workspace-tests", "cargo", "test", "--workspace", "--locked")
    run("fixture", "python3", "tools/create-preflight-fixture.py")
    run("package", "python3", "tools/package-dev.py")
    helper = Path("dist/GlidePod.app/Contents/MacOS/glidepodctl")
    probe = run("probe", helper, "--json", "probe")
    (EVIDENCE / "host-probes.json").write_text(probe.stdout)
    apk = run("apk-preflight", helper, "--json", "inspect-apk", ".cache/fixtures/GlidePod-Preflight.apk")
    (EVIDENCE / "apk-preflight.json").write_text(apk.stdout)
    with tempfile.TemporaryDirectory(prefix="glidepod-validation-") as directory:
        command = [helper, "--data-dir", directory, "--json"]
        state = json.loads(run("cli-status", *command, "status").stdout)
        assert not state["backend_ready"] and state["installed_apps"] == 0
        start = run("cli-start-unavailable", *command, "start", expected=1)
        assert not start.stdout and "error" in json.loads(start.stderr)
        run("cli-profile", *command, "profile", "developer")
        clear = run("cli-clear-data-confirmation", *command, "clear-storage", "dev.example.fixture", "--data", expected=1)
        assert not clear.stdout and "--confirm" in json.loads(clear.stderr)["error"]
        changed = json.loads(run("cli-configure", *command, "configure", "--memory-mib", "8192").stdout)
        assert changed["developer"]["memory_mib"] == 8192 and changed["personal"]["memory_mib"] == 4096
        invalid = run("cli-invalid-resources", *command, "configure", "--memory-mib", "1", expected=1)
        assert not invalid.stdout
        final = json.loads(run("cli-final-status", *command, "status").stdout)
        assert final["configuration"]["revision"] == changed["revision"]
    run("bundle-signature", "codesign", "--verify", "--deep", "--strict", "dist/GlidePod.app")
    bundle = ROOT / "dist/GlidePod.app/Contents"
    info = plistlib.loads((bundle / "Info.plist").read_bytes())
    assert info["CFBundleIconName"] == "GlidePod" and info["CFBundleIconFile"] == "GlidePod"
    for resource in ("Assets.car", "GlidePod.icns"):
        assert (bundle / "Resources" / resource).stat().st_size > 1024
    run("helper-entitlements", "codesign", "-d", "--entitlements", ":-", helper)
    run("ui-entitlements", "codesign", "-d", "--entitlements", ":-", "dist/GlidePod.app")
    run("deployment-target", "vtool", "-show-build", "dist/GlidePod.app/Contents/MacOS/glidepod")
    summary = {
        "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "local G0 host foundation",
        "rustc": run("toolchain", "rustc", "--version").stdout.strip(),
        "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "workspace_checks_passed": True,
        "cli_contracts_passed": True,
        "ad_hoc_signature_verified": True,
        "production_runtime_qualified": False,
        "experimental_runtime_management_implemented": True,
        "experimental_runtime_tested_by_this_runner": False,
        "experimental_runtime_evidence": "../g1/runtime-session/report.json",
        "separate_android_boot_evidence": "android-boot/report.json; produced by tools/boot-experiment.py, not this runner",
        "android_gpu_verified_by_this_runner": False,
        "app_session_evidence": ["../g2/software-session/app-session.json", "../g2/metal-session/app-session.json"],
        "window_input_evidence": ["../g3/software-input/report.json", "../g3/metal-input/report.json", "../g3/host-focus/report.json"],
        "window_input_tested_by_this_runner": False,
        "management_evidence": "../g4/management/report.json",
        "guest_3d_evidence": ["../g4/3d/metal/report.json", "../g4/3d/software/report.json"],
        "native_notification_banner_verified": False,
        "physical_input_devices_verified": False,
        "native_window_buttons_verified": False,
        "g0_complete": False,
        "layered_app_icon_bundled": True,
        "icon_assets_sha256": hashlib.sha256((bundle / "Resources/Assets.car").read_bytes()).hexdigest(),
        "binaries": {name: hashlib.sha256((ROOT / "dist/GlidePod.app/Contents/MacOS" / name).read_bytes()).hexdigest() for name in ("glidepod", "glidepodctl")},
    }
    (EVIDENCE / "validation.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))

if __name__ == "__main__":
    main()
