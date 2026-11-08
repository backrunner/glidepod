#!/usr/bin/env python3
"""Run host topology/context regressions against an exact source-built ANGLE artifact.

This checks host GLES correctness with Metal API Validation. Android, game
compatibility and performance acceptance require their separate VM checks.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifact', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    artifact, out = args.artifact.resolve(), args.output.resolve()
    provenance = json.loads((artifact / 'source.json').read_text())
    lock = json.loads((ROOT / 'engine/angle-metal-lock.json').read_text())
    if provenance['lock'] != lock:
        parser.error('Artifact does not match the current source lock')
    for name, expected in provenance['libraries'].items():
        if digest(artifact / name) != expected:
            parser.error(f'Artifact library changed: {name}')
    out.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, DYLD_LIBRARY_PATH=str(artifact),
               ANGLE_DEFAULT_PLATFORM='metal', MTL_DEBUG_LAYER='1')
    env.pop('ANGLE_FEATURE_OVERRIDES_ENABLED', None)
    env.pop('ANGLE_FEATURE_OVERRIDES_DISABLED', None)
    include = ROOT / '.cache/upstream/angle-metal/include'
    results = {}
    for name in ('gles-topology', 'gles-contexts', 'gles-readback'):
        command = ['clang++', '-std=c++17', '-O2', '-Wall', '-Wextra', '-Werror',
                   '-I', str(include), str(ROOT / 'tools' / f'{name}.cpp'),
                   '-L', str(artifact), '-lEGL', '-lGLESv2', '-o', str(out / name)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=60)
        (out / f'{name}-build.log').write_text(result.stdout + result.stderr)
        result.check_returncode()
    for name, binary, arguments in [('topology', 'gles-topology', []),
                                     ('contexts', 'gles-contexts', []),
                                     ('contexts-shared', 'gles-contexts', ['--shared']),
                                     ('readback', 'gles-readback', [])]:
        result = subprocess.run([str(out / binary), *arguments], env=env,
                                capture_output=True, text=True, timeout=180)
        (out / f'{name}.json').write_text(result.stdout)
        (out / f'{name}.log').write_text(result.stderr)
        result.check_returncode()
        report = json.loads(result.stdout)
        if report.get('passed') is not True:
            raise RuntimeError(f'{name}: correctness failed')
        if 'Metal API Validation Enabled' not in result.stderr:
            raise RuntimeError(f'{name}: Metal API Validation was not enabled')
        results[name] = report
    summary = {'passed': True, 'artifact': str(artifact),
               'libraries': provenance['libraries'], 'metal_api_validation': True,
               'topology_cases': len(results['topology']['checks']),
               'context_iterations': results['contexts']['iterations'],
               'shared_context_iterations': results['contexts-shared']['iterations'],
               'readback_checks': results['readback']['checks'],
               'android_vm_verified': False, 'performance_comparison': False}
    (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
