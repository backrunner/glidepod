#!/usr/bin/env python3
"""Verify simultaneous direct Vulkan/GLES rendering on separate guest displays."""
import argparse
import concurrent.futures
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/debug/glidepodctl')
    args = parser.parse_args()
    data, out = args.data_dir.resolve(), args.output.resolve()
    assert data.is_relative_to(ROOT / '.cache')
    assert out.is_relative_to(ROOT / 'evidence') and not out.exists()
    out.mkdir(parents=True)
    cli = [str(args.cli.resolve()), '--data-dir', str(data), '--json']
    for package in ('dev.glidepod.vulkan', 'dev.glidepod.benchmark'):
        subprocess.run([*cli, 'shell', f'am force-stop {package}'], check=True, stdout=subprocess.DEVNULL)

    def run(api):
        command = [*cli, 'benchmark', '--api', api, '--samples', '600' if api == 'vulkan' else '180']
        if api == 'gles':
            command += ['--gles', '3']
        started = time.monotonic()
        result = subprocess.run(command, capture_output=True, text=True, timeout=180)
        ended = time.monotonic()
        (out / f'{api}.log').write_text(result.stderr)
        if result.returncode:
            raise RuntimeError(result.stderr)
        (out / f'{api}.json').write_text(result.stdout)
        return json.loads(result.stdout), started, ended

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        first = pool.submit(run, 'vulkan')
        # APK mutations are intentionally serialized. Start the second install only
        # after the first package has launched and released the installer lock.
        deadline = time.monotonic() + 45
        while True:
            if first.done():
                first.result()
                raise AssertionError('Vulkan workload ended before the second app started')
            pid = subprocess.run([*cli, 'shell', 'pidof dev.glidepod.vulkan'], capture_output=True, text=True)
            if pid.returncode == 0 and json.loads(pid.stdout).get('stdout', '').strip():
                break
            if time.monotonic() >= deadline:
                raise TimeoutError('Vulkan package did not start')
            time.sleep(.2)
        second = pool.submit(run, 'gles')
        a, b = first.result(), second.result()
    vk, gl = a[0], b[0]
    assert a[1] < b[2] and b[1] < a[2]
    assert vk['epoch'] == gl['epoch'] and vk['renderer'] == gl['renderer']
    assert vk['display_id'] != gl['display_id'] and min(vk['display_id'], gl['display_id']) > 0
    assert min(vk['host_frames'], gl['host_frames']) > 20
    assert vk['guest']['vk_success'] and gl['guest']['gles3_probe']['correctness']['passed']
    report = {'passed': True, 'renderer': vk['renderer'], 'epoch': vk['epoch'],
              'display_ids': [vk['display_id'], gl['display_id']],
              'host_frames': [vk['host_frames'], gl['host_frames']],
              'invocation_overlap_seconds': min(a[2], b[2]) - max(a[1], b[1]),
              'native_macos_windows_visually_verified': False,
              'performance_comparison': False}
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
