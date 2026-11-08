#!/usr/bin/env python3
"""Build the reviewed memory patch against the pinned external VMM; no image redistribution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], cwd=ROOT, check=True, **kwargs)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', type=Path, help='Register after building; the selected runtime must be stopped')
    parser.add_argument('--host-services', action='store_true', help='Also build GNSS validation and the vmnet bridge backend')
    parser.add_argument('--gles-metal', action='store_true', help='Build the ES 3.0 host GLES experiment; requires the separately built ANGLE Metal libraries at launch')
    args = parser.parse_args()
    lock = json.loads((ROOT / 'engine/upstream-lock.json').read_text())
    base = ROOT / '.cache/upstream/avo'
    patches = [ROOT / 'engine/patches/avo-adaptive-memory.patch']
    if args.host_services:
        patches.append(ROOT / 'engine/patches/avo-host-services.patch')
    if args.gles_metal:
        patches.append(ROOT / 'engine/patches/avo-direct-gles-metal-probe.patch')
    # Preserve binaries referenced by existing stores when the patch changes.
    patch_hash = hashlib.sha256(b''.join(p.read_bytes() for p in patches)).hexdigest()
    kind = 'gles-metal' if args.gles_metal else 'host' if args.host_services else 'memory'
    source = ROOT / f'.cache/upstream/avo-{kind}-build-{patch_hash[:12]}'
    if not base.exists():
        run('git', 'clone', lock['repository'], base)
        run('git', '-C', base, 'checkout', '--detach', lock['revision'])
    assert run('git', '-C', base, 'rev-parse', 'HEAD', capture_output=True, text=True).stdout.strip() == lock['revision']
    if not source.exists():
        run('git', 'clone', '--shared', '--no-hardlinks', base, source)
    assert run('git', '-C', source, 'rev-parse', 'HEAD', capture_output=True, text=True).stdout.strip() == lock['revision']
    combined = source.parent / f'glidepod-engine-{patch_hash}.patch'
    combined.write_bytes(b''.join(p.read_bytes() for p in patches))
    applied = subprocess.run(['git', '-C', str(source), 'apply', '--reverse', '--check', str(combined)], capture_output=True).returncode == 0
    if not applied:
        assert not run('git', '-C', source, 'status', '--porcelain', capture_output=True, text=True).stdout.strip(), 'Preserve modified engine checkout; inspect it before building'
        run('git', '-C', source, 'apply', '--check', combined)
        run('git', '-C', source, 'apply', combined)
    # Compare every source byte against HEAD + this exact patch without changing the real index.
    with tempfile.TemporaryDirectory(prefix='glidepod-engine-index-') as temp:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(temp) / 'index'))
        run('git', '-C', source, 'read-tree', 'HEAD', env=env)
        run('git', '-C', source, 'apply', '--cached', combined, env=env)
        run('git', '-C', source, 'diff', '--exit-code', env=env)
        assert not run('git', '-C', source, 'ls-files', '--others', '--exclude-standard', capture_output=True, text=True, env=env).stdout.strip(), 'Unexpected engine source files; inspect before building'
    run('cargo', 'build', '--release', '--locked', '--manifest-path', source / 'engine/hvf/Cargo.toml', '-p', 'avo-vmm')
    engine = source / 'engine/hvf/target/release/avo-vmm'
    run('codesign', '--force', '--sign', '-', '--entitlements', ROOT / 'macos/packaging/hypervisor.entitlements', engine)
    provenance = {'engine': str(engine), 'base_revision': lock['revision'], 'patch_sha256': patch_hash, 'patches': [{'path': str(p.relative_to(ROOT)), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in patches], 'sha256': hashlib.sha256(engine.read_bytes()).hexdigest()}
    engine.with_suffix('.provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    print(json.dumps(provenance, indent=2))
    if args.data_dir:
        images = ROOT / '.cache/cuttlefish' / json.loads((ROOT / 'engine/cuttlefish-lock.json').read_text())['build_id'] / 'images'
        run(ROOT / 'target/debug/glidepodctl', '--data-dir', args.data_dir, 'register-experimental', '--engine', engine, '--images', images,
            '--fingerprint', 'generic/aosp_cf_arm64_only_phone/vsoc_arm64_only:17/CP2A.260605.016/16644040:userdebug/test-keys')

if __name__ == '__main__':
    main()
