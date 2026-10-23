#!/usr/bin/env python3
"""Boot the separately built candidate and locked AOSP image in software mode.

All disks and sockets stay under .cache. No engine is added to the product bundle.
"""
import hashlib
import datetime
import fcntl
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import socket
import subprocess
import time
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    return subprocess.run([*map(str, args)], cwd=ROOT, check=True, capture_output=True, text=True)

def main():
    os.umask(0o077)
    experiment = ROOT / ".cache/android-boot"
    experiment.mkdir(parents=True, exist_ok=True)
    process_lock = (experiment / "experiment.lock").open("w")
    try:
        fcntl.flock(process_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise SystemExit("Another boot experiment is already using this instance")
    lock = json.loads((ROOT / "engine/cuttlefish-lock.json").read_text())
    upstream = json.loads((ROOT / "engine/upstream-lock.json").read_text())
    checkout = ROOT / ".cache/upstream/avo"
    assert run("git", "-C", checkout, "rev-parse", "HEAD").stdout.strip() == upstream["revision"]
    assert not run("git", "-C", checkout, "status", "--porcelain").stdout.strip(), "Upstream checkout is modified"
    source = ROOT / ".cache/cuttlefish" / lock["build_id"]
    archive = next(a for a in lock["artifacts"] if a["name"].endswith(".zip"))
    path = source / archive["name"]
    with path.open("rb") as file:
        assert hashlib.file_digest(file, "sha256").hexdigest() == archive["sha256"]
    images = source / "images"
    images.mkdir(exist_ok=True)
    with zipfile.ZipFile(path) as zipped:
        entries = zipped.infolist()
        assert len(entries) <= 64 and sum(i.file_size for i in entries) <= 32 << 30
        assert len({i.filename for i in entries}) == len(entries)
        for entry in entries:
            assert PurePosixPath(entry.filename).name == entry.filename and entry.filename not in (".", "..")
            assert (entry.external_attr >> 16) & 0o170000 != 0o120000
            target = images / entry.filename
            crc = 0
            cached = target.is_file() and not target.is_symlink() and target.stat().st_size == entry.file_size
            if cached:
                with target.open("rb") as existing:
                    for block in iter(lambda: existing.read(4 << 20), b""):
                        crc = zlib.crc32(block, crc)
            if target.is_symlink():
                raise SystemExit("Extracted image must not be a symlink")
            if not cached or crc != entry.CRC:
                with zipped.open(entry) as reader, target.open("wb") as writer:
                    shutil.copyfileobj(reader, writer, 4 << 20)
    run("cargo", "build", "--release", "--locked", "--manifest-path", checkout / "engine/hvf/Cargo.toml", "-p", "avo-vmm")
    engine = checkout / "engine/hvf/target/release/avo-vmm"
    run("codesign", "--force", "--sign", "-", "--entitlements", ROOT / "macos/packaging/hypervisor.entitlements", engine)
    runtime = experiment / "runtime"
    runtime.mkdir(exist_ok=True)
    evidence = ROOT / "evidence/g0/android-boot"
    evidence.mkdir(parents=True, exist_ok=True)
    previous_report = evidence / "report.json"
    if previous_report.exists() and not (evidence / "first-boot-report.json").exists():
        shutil.copy2(previous_report, evidence / "first-boot-report.json")
    snapshot = evidence / "android-settings.bmp"
    if snapshot.exists():
        snapshot.unlink()
    command = [str(engine), "cuttlefish", str(images), "--instance", str(experiment / "instance"), "--runtime", str(runtime), "--gpu", "2d", "--cpus", "4", "--mem", "4096", "--display", "720x1280", "--timeout", "120", "--exit-on-reboot", "--no-microphone", "--cameras", "none"]
    started = time.monotonic()
    console = None
    output = bytearray()
    booted = False
    last_query = 0
    completed_at = None
    snapshot_requested = False
    shutdown_requested = False
    shutdown_at = None
    control_reply = ""
    with (evidence / "engine.log").open("wb") as log:
        process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, env={**os.environ, "AVO_VMM_MUTE": "1"})
        print(f"Boot experiment started (build {lock['build_id']}, software rendering)", flush=True)
        try:
            while process.poll() is None and time.monotonic() - started < 125:
                if console is None and (runtime / "console.sock").exists():
                    candidate = socket.socket(socket.AF_UNIX)
                    candidate.settimeout(0.25)
                    try:
                        candidate.connect(str(runtime / "console.sock"))
                        console = candidate
                    except (ConnectionRefusedError, FileNotFoundError):
                        # A previous process may have left its socket pathname.
                        candidate.close()
                if console:
                    now = time.monotonic()
                    if now - last_query > 3 and not booted:
                        console.sendall(b"\necho GLIDEPOD_BOOT_$(getprop sys.boot_completed)\n")
                        last_query = now
                    try:
                        data = console.recv(8192)
                        if not data:
                            break
                        output.extend(data)
                    except TimeoutError:
                        pass
                    if not booted and re.search(rb"(?:\r?\n)GLIDEPOD_BOOT_1\r?\n", output):
                        booted = True
                        completed_at = time.monotonic() - started
                        print(f"Android boot completed in {completed_at:.1f} seconds", flush=True)
                        console.sendall(b"echo GLIDEPOD_FINGERPRINT_$(getprop ro.build.fingerprint); echo GLIDEPOD_SDK_$(getprop ro.build.version.sdk); pm path android; wm size; wm dismiss-keyguard; am start -a android.settings.SETTINGS\n")
                    if booted and not snapshot_requested and time.monotonic() - started > completed_at + 5:
                        with socket.socket(socket.AF_UNIX) as control:
                            control.settimeout(2)
                            control.connect(str(runtime / "control.sock"))
                            control.sendall(f"snapshot 0 {snapshot}\n".encode())
                            control_reply = control.recv(4096).decode(errors="replace").strip()
                        snapshot_requested = True
                    if snapshot_requested and not shutdown_requested and snapshot.exists() and snapshot.stat().st_size > 1_000_000:
                        console.sendall(b"reboot -p\n")
                        shutdown_requested = True
                        shutdown_at = time.monotonic()
                    if shutdown_at and time.monotonic() - shutdown_at > 20:
                        break
                    if len(output) > 1024 * 1024:
                        raise RuntimeError("Console output exceeded experiment bound")
                else:
                    time.sleep(0.2)
        finally:
            if console:
                console.close()
            if shutdown_requested and process.poll() is None:
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
    (evidence / "console.txt").write_bytes(output)
    for name in ("kernel.log", "console.log", "logcat.log", "bootconfig.txt"):
        source_log = experiment / "instance" / name
        if source_log.exists():
            shutil.copy2(source_log, evidence / name)
    frame_verified = snapshot.exists() and snapshot.stat().st_size > 1_000_000
    if frame_verified:
        run("sips", "-s", "format", "png", snapshot, "--out", evidence / "android-settings.png")
    report = {"build_id": lock["build_id"], "engine_revision": upstream["revision"], "software_rendering": True, "android_boot_completed": booted, "boot_seconds": completed_at, "package_service_verified": b"package:/system/framework/framework-res.apk" in output, "snapshot_command_reply": control_reply, "display_frame_captured": frame_verified, "shutdown_requested": shutdown_requested, "graceful_shutdown": shutdown_requested and process.returncode == 0, "android_gpu_verified": False, "engine_bundled": False, "exit_code": process.returncode, "process_reaped": process.poll() is not None}
    report["recorded_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["image_sha256"] = archive["sha256"]
    report["resources"] = {"cpus": 4, "memory_mib": 4096, "display": "720x1280"}
    report["timing_method"] = "VMM process spawn to observed boot_completed=1, queried every 3 seconds; not a benchmark"
    (evidence / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    if not booted or not frame_verified or not report["graceful_shutdown"]:
        raise SystemExit(1)

if __name__ == "__main__":
    main()
