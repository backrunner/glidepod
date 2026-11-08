#!/usr/bin/env python3
"""Build the pinned, patched ANGLE/gfxstream Metal experiment without changing user settings.

All sources and outputs stay in the project cache. Existing modified source or
artifact directories are verified, never reset or overwritten silently.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def run(*args, cwd=ROOT, **kwargs):
    return subprocess.run([str(arg) for arg in args], cwd=cwd, check=True, **kwargs)


def checkout(path, repository, revision):
    if not path.exists():
        path.mkdir(parents=True)
        run('git', 'init', path)
        run('git', '-C', path, 'remote', 'add', 'origin', repository)
        run('git', '-C', path, 'fetch', '--depth', '1', 'origin', revision)
        run('git', '-C', path, 'checkout', '--detach', revision)
    actual = run('git', '-C', path, 'rev-parse', 'HEAD', capture_output=True, text=True).stdout.strip()
    if actual != revision:
        raise RuntimeError(f'Preserve existing checkout {path}: expected {revision}, found {actual}')


def apply_verified_patch(source, patch):
    applied = subprocess.run(['git', '-C', str(source), 'apply', '--reverse', '--check', str(patch)],
                             capture_output=True).returncode == 0
    if not applied:
        if run('git', '-C', source, 'status', '--porcelain', capture_output=True, text=True).stdout.strip():
            raise RuntimeError('Preserve modified source checkout; inspect it before building')
        run('git', '-C', source, 'apply', '--check', patch)
        run('git', '-C', source, 'apply', patch)
    with tempfile.TemporaryDirectory(prefix='glidepod-angle-index-') as temporary:
        env = dict(os.environ, GIT_INDEX_FILE=str(Path(temporary) / 'index'))
        run('git', '-C', source, 'read-tree', 'HEAD', env=env)
        run('git', '-C', source, 'apply', '--cached', patch, env=env)
        run('git', '-C', source, 'diff', '--exit-code', env=env)
        if run('git', '-C', source, 'ls-files', '--others', '--exclude-standard',
               capture_output=True, text=True, env=env).stdout.strip():
            raise RuntimeError('Unexpected source files; inspect before building')


def main():
    lock = json.loads((ROOT / 'engine/angle-metal-lock.json').read_text())
    patches = [ROOT / entry['path'] for entry in lock['patches']]
    for path, entry in zip(patches, lock['patches']):
        if digest(path) != entry['sha256']:
            raise RuntimeError(f'ANGLE patch does not match its lock: {path}')
    combined = b''.join(path.read_bytes() for path in patches)
    source = ROOT / '.cache/upstream/angle-metal'
    source.parent.mkdir(parents=True, exist_ok=True)
    patch = source.parent / f'glidepod-angle-{hashlib.sha256(combined).hexdigest()}.patch'
    patch.write_bytes(combined)
    depot = ROOT / '.cache/upstream/depot_tools'
    checkout(source, lock['repository'], lock['revision'])
    checkout(depot, lock['depot_tools_repository'], lock['depot_tools_revision'])
    apply_verified_patch(source, patch)
    config = {'name': '.', 'url': lock['repository'], 'managed': False,
              'custom_vars': {'checkout_angle_cl_deps': False, 'checkout_angle_dawn_deps': False},
              'custom_deps': {'third_party/SwiftShader': None, 'third_party/VK-GL-CTS/src': None}}
    configuration = 'solutions = ' + repr([config]) + '\n'
    gclient = source / '.gclient'
    if gclient.exists() and gclient.read_text() != configuration:
        raise RuntimeError('Preserve different .gclient configuration')
    gclient.write_text(configuration)
    env = dict(os.environ, PATH=str(depot) + os.pathsep + os.environ['PATH'], DEPOT_TOOLS_UPDATE='0')
    # Rust IR is disabled by the lock; no unrelated hook/toolchain installation is needed.
    run(depot / 'gclient', 'sync', '--no-history', '--nohooks', cwd=source, env=env)
    build = source / 'out/Metal'
    build.mkdir(parents=True, exist_ok=True)
    (build / 'args.gn').write_text(lock['gn_args'])
    run(source / 'buildtools/mac/gn', 'gen', 'out/Metal', cwd=source, env=env)
    run(source / 'third_party/ninja/ninja', '-C', 'out/Metal', '-j12', 'libEGL', 'libGLESv2', cwd=source, env=env)
    gfx = lock['gfxstream']
    gfx_source = ROOT / '.cache/upstream/gfxstream-direct-metal'
    gfx_build = ROOT / '.cache/upstream/gfxstream-direct-metal-build'
    gfx_patch = ROOT / gfx['patch']
    if digest(gfx_patch) != gfx['patch_sha256']:
        raise RuntimeError('gfxstream patch does not match its lock')
    checkout(gfx_source, gfx['repository'], gfx['revision'])
    apply_verified_patch(gfx_source, gfx_patch)
    run('cmake', '-S', gfx_source, '-B', gfx_build, *gfx['cmake_args'])
    run('cmake', '--build', gfx_build, '--target', 'gfxstream_backend', '--parallel', '12')
    built = {name: build / name for name in ('libEGL.dylib', 'libGLESv2.dylib')}
    built['libgfxstream_backend.dylib'] = gfx_build / 'libgfxstream_backend.dylib'
    libraries = {name: digest(path) for name, path in built.items()}
    identity = hashlib.sha256(json.dumps({'lock': lock, 'libraries': libraries}, sort_keys=True).encode()).hexdigest()[:16]
    out = ROOT / '.cache/direct-gles-metal' / identity
    if out.exists():
        provenance = json.loads((out / 'source.json').read_text())
        if (provenance['libraries'] != libraries or provenance['lock'] != lock
                or provenance['deps_entries_sha256'] != digest(source / '.gclient_entries')):
            raise RuntimeError('Cached ANGLE provenance changed')
        for name, expected in provenance['files'].items():
            if digest(out / name) != expected:
                raise RuntimeError(f'Cached ANGLE artifact changed: {name}')
    else:
        out.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.angle-build-', dir=out.parent) as temporary:
            stage = Path(temporary)
            for name in libraries:
                shutil.copy2(built[name], stage / name)
            # Preserve upstream and dependency notices with the local artifact.
            files = dict(libraries)
            for label, notice_source in [('angle', source), ('gfxstream', gfx_source)]:
                for directory, dirs, names in os.walk(notice_source):
                    dirs[:] = [d for d in dirs if d not in {'.git', 'out', '.cipd', '__pycache__'}]
                    for name in names:
                        if not name.upper().startswith(('LICENSE', 'NOTICE', 'COPYING')):
                            continue
                        path = Path(directory) / name
                        if path.is_symlink() or not path.is_file():
                            continue
                        relative = Path('notices') / label / path.relative_to(notice_source)
                        target = stage / relative
                        target.parent.mkdir(parents=True, exist_ok=True)
                        shutil.copy2(path, target)
                        files[str(relative)] = digest(target)
            provenance = {'lock': lock, 'libraries': libraries, 'files': files,
                          'deps_entries_sha256': digest(source / '.gclient_entries'),
                          'xcode': run('xcodebuild', '-version', capture_output=True, text=True).stdout.strip(),
                          'qualified_games': False, 'automatic_default': False}
            (stage / 'source.json').write_text(json.dumps(provenance, indent=2) + '\n')
            # Publish the complete immutable directory only after all files exist.
            os.rename(stage, out)
    print(json.dumps({'directory': str(out), 'libraries': libraries,
                      'environment': {'DYLD_LIBRARY_PATH': str(out), 'ANGLE_DEFAULT_PLATFORM': 'metal',
                                      'ANDROID_EGL_ON_EGL': '1'},
                      'gles_version': '3.0', 'automatic_default': False}, indent=2))


if __name__ == '__main__':
    main()
