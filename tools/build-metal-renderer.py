#!/usr/bin/env python3
"""Build the pinned experimental Metal renderer into an immutable local cache directory."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache"
LOCK = ROOT / "engine/metal-renderer-lock.json"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(*args, cwd=ROOT, log=None):
    r = subprocess.run([*map(str, args)], cwd=cwd,
                       stdout=log or subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if r.returncode:
        raise RuntimeError(f"Command failed ({r.returncode}): {' '.join(map(str, args))}\n{r.stdout or 'See build log.'}")
    return r.stdout or ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-renderer", type=Path, default=CACHE / "upstream/avo/build/gfxstream")
    args = parser.parse_args()
    lock = json.loads(LOCK.read_text())
    mvk = lock["moltenvk"]
    patch = ROOT / mvk["patch"]
    assert sha(patch) == mvk["patch_sha256"], "Patch differs from lock"
    for f in lock["base_renderer"]["files"]:
        assert sha(args.base_renderer / f["path"]) == f["sha256"], f"Unreviewed base renderer: {f['path']}"
    identity = sha(LOCK)[:16]
    output = CACHE / "metal-renderer" / identity
    if output.exists():
        record = json.loads((output / "build.json").read_text())
        assert record["lock_sha256"] == sha(LOCK)
        for name, digest in record["files"].items():
            assert sha(output / name) == digest, f"Changed cached renderer: {name}"
        print(output)
        return
    source = CACHE / "upstream/MoltenVK-gles3-v142"
    if not source.exists():
        run("git", "init", "-q", source)
        run("git", "remote", "add", "origin", mvk["repository"], cwd=source)
        run("git", "fetch", "--depth", "1", "origin", mvk["revision"], cwd=source)
        run("git", "checkout", "--detach", "FETCH_HEAD", cwd=source)
    assert run("git", "rev-parse", "HEAD", cwd=source).strip() == mvk["revision"]
    diff = run("git", "diff", "HEAD", "--binary", cwd=source)
    if not diff:
        run("git", "apply", "--check", patch, cwd=source)
        run("git", "apply", patch, cwd=source)
        diff = run("git", "diff", "HEAD", "--binary", cwd=source)
    assert diff == patch.read_text(), "Source contains changes beyond the reviewed patch"
    assert not run("git", "ls-files", "--others", "--exclude-standard", cwd=source).strip(), "Untracked source inputs"
    for name, revision in mvk["dependencies"].items():
        assert (source / "ExternalRevisions" / f"{name}_repo_revision").read_text().strip() == revision
    cpm_cache = CACHE / "moltenvk-cpm"
    cpm = cpm_cache / "cpm/CPM_0.40.8.cmake"
    cpm.parent.mkdir(parents=True, exist_ok=True)
    if not cpm.exists():
        with urllib.request.urlopen(lock["cpm"]["url"], timeout=60) as response:
            data = response.read(512 * 1024)
        assert hashlib.sha256(data).hexdigest() == lock["cpm"]["sha256"]
        cpm.write_bytes(data)
    assert sha(cpm) == lock["cpm"]["sha256"], "Changed CPM build input"
    evidence = ROOT / "evidence/g5/build"
    evidence.mkdir(parents=True, exist_ok=True)
    build = CACHE / "moltenvk-gles3-v142-build"
    with (evidence / "configure.log").open("w") as log:
        run("cmake", "-S", source, "-B", build, "-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_OSX_ARCHITECTURES=arm64", "-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0",
            "-DMVK_USE_METAL_PRIVATE_API=ON", "-DMVK_EXCLUDE_SPIRV_TOOLS=ON",
            f"-DCPM_SOURCE_CACHE={cpm_cache}", cwd=source, log=log)
    with (evidence / "compile.log").open("w") as log:
        run("cmake", "--build", build, "--parallel", "10", cwd=source, log=log)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="renderer-stage-", dir=output.parent) as directory:
        stage = Path(directory) / "renderer"
        for f in lock["base_renderer"]["files"]:
            target = stage / f["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(args.base_renderer / f["path"], target)
        library = stage / "lib64/vulkan/libMoltenVK.dylib"
        shutil.copy2(build / "MoltenVK/libMoltenVK.1.4.2.dylib", library)
        run("codesign", "--force", "--sign", "-", library)
        run("codesign", "--verify", "--strict", library)
        shutil.copytree(args.base_renderer / "licenses", stage / "licenses")
        shutil.copy2(source / "LICENSE", stage / "licenses/MoltenVK-LICENSE")
        for name, revision in mvk["dependencies"].items():
            dirs = list((cpm_cache / name.lower()).glob(f"*/{name}"))
            candidates = [p for p in dirs if run("git", "rev-parse", "HEAD", cwd=p).strip() == revision]
            assert len(candidates) == 1, f"Cannot identify dependency {name}"
            for notice in candidates[0].iterdir():
                if notice.is_file() and notice.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")):
                    shutil.copy2(notice, stage / "licenses" / f"{name}-{notice.name}")
        (stage / "source.properties").write_text(
            "Pkg.Desc=GlidePod experimental gfxstream and MoltenVK renderer\n"
            f"Pkg.Revision=glidepod.gles3.{identity}\n"
            f"MoltenVK.Commit={mvk['revision']}\nMoltenVK.PrivateAPI=true\n"
            f"MoltenVK.PatchSHA256={mvk['patch_sha256']}\n")
        record = {"lock_sha256": sha(LOCK), "lock": lock,
                  "toolchain": run("xcodebuild", "-version").strip(),
                  "files": {str(p.relative_to(stage)): sha(p) for p in sorted(stage.rglob("*")) if p.is_file()}}
        (stage / "build.json").write_text(json.dumps(record, indent=2) + "\n")
        stage.rename(output)
    shutil.copy2(output / "build.json", evidence / "provenance.json")
    print(output)


if __name__ == "__main__":
    main()
