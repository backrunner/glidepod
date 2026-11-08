#!/usr/bin/env python3
"""Prepare the pinned SDK driver with the evaluated gfxstream/loader, without installing it globally."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sdk', type=Path, required=True, help='Locally extracted SDK root containing macOS/')
    parser.add_argument('--base-renderer', type=Path, default=ROOT / '.cache/upstream/avo/build/gfxstream')
    args = parser.parse_args()
    lock_path = ROOT / 'engine/kosmickrisp-renderer-lock.json'
    base_lock_path = ROOT / 'engine/metal-renderer-lock.json'
    lock = json.loads(lock_path.read_text())
    base = json.loads(base_lock_path.read_text())['base_renderer']
    library = args.sdk / lock['driver_file']
    notice = args.sdk / 'Licenses/LICENSE.txt'
    assert sha(library) == lock['driver_sha256'], 'SDK driver differs from lock'
    assert sha(notice) == lock['sdk_notice_sha256'], 'SDK notice differs from lock'
    files = [f for f in base['files'] if f['path'].endswith('.dylib')]
    assert len(files) == 2
    for f in files:
        assert sha(args.base_renderer / f['path']) == f['sha256'], f"Changed base artifact: {f['path']}"
    identity = hashlib.sha256((sha(lock_path) + sha(base_lock_path)).encode()).hexdigest()[:16]
    output = ROOT / '.cache/kosmickrisp-renderer' / identity
    if output.exists():
        record = json.loads((output / 'build.json').read_text())
        assert record['identity'] == identity
        for name, digest in record['files'].items():
            assert sha(output / name) == digest, f'Changed cached artifact: {name}'
        print(output)
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='renderer-stage-', dir=output.parent) as directory:
        stage = Path(directory) / 'renderer'
        for f in files:
            target = stage / f['path']
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(args.base_renderer / f['path'], target)
        shutil.copy2(library, stage / 'lib64/vulkan/libvulkan_kosmickrisp.dylib')
        (stage / 'lib64/vulkan/libkosmickrisp_icd.json').write_text(json.dumps({
            'file_format_version': '1.0.0',
            'ICD': {'api_version': '1.4.0', 'library_path': './libvulkan_kosmickrisp.dylib'},
        }, indent=2) + '\n')
        shutil.copytree(args.base_renderer / 'licenses', stage / 'licenses')
        shutil.copy2(notice, stage / 'licenses/LunarG-SDK-NOTICE.txt')
        (stage / 'source.properties').write_text(
            'Pkg.Desc=GlidePod experimental gfxstream and KosmicKrisp renderer\n'
            f'Pkg.Revision=glidepod.kosmickrisp.{identity}\n'
            f"KosmicKrisp.SDK={lock['sdk_version']}\nKosmicKrisp.SHA256={lock['driver_sha256']}\n")
        record = {
            'identity': identity, 'lock': lock, 'base_renderer': base,
            'lock_sha256': sha(lock_path), 'base_lock_sha256': sha(base_lock_path),
            'driver_origin': 'LunarG SDK binary; not source-built by GlidePod',
            'files': {str(p.relative_to(stage)): sha(p) for p in sorted(stage.rglob('*')) if p.is_file()},
        }
        (stage / 'build.json').write_text(json.dumps(record, indent=2) + '\n')
        stage.rename(output)
    print(output)


if __name__ == '__main__':
    main()
