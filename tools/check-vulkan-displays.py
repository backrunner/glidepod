#!/usr/bin/env python3
"""Verify simultaneous direct Vulkan/GLES rendering on separate guest displays."""
import argparse
import concurrent.futures
import json
import re
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def ready_vulkan_window(windows, previous_tokens):
    for block in re.split(r'(?m)^  Window #', windows):
        header = block.split('\n', 1)[0]
        token = re.search(r'Window\{([0-9a-f]+)\s', header)
        if (token and token.group(1) not in previous_tokens and ' EXITING' not in header
                and 'dev.glidepod.vulkan/dev.glidepod.vulkan.MainActivity' in header
                and 'mHasSurface=true isReadyForDisplay()=true' in block
                and 'mDrawState=HAS_DRAWN' in block):
            return True
    return False


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
    # Android can relaunch other activities when an installation updates asset
    # paths. Install both fixtures before either measured activity is running;
    # benchmark() then reuses them only after checking the exact APK hashes.
    installations = []
    for name in ('glidepod-vulkan.apk', 'glidepod-benchmark.apk'):
        installations.append(json.loads(subprocess.check_output(
            [*cli, 'install', str(ROOT / 'target/android' / name)], text=True)))
    (out / 'fixture-installation.json').write_text(json.dumps(installations, indent=2) + '\n')
    initial = json.loads(subprocess.check_output([*cli, 'shell', 'dumpsys window windows'], text=True))['stdout']
    previous_tokens = set(re.findall(r'Window\{([0-9a-f]+)\s', initial))
    (out / 'previous-windows.txt').write_text(initial)

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
        # The result provider starts the process before its activity/display exists.
        # Wait for a drawn window, not merely a PID, before adding another display.
        deadline = time.monotonic() + 45
        while True:
            if first.done():
                first.result()
                raise AssertionError('Vulkan workload ended before the second app started')
            state = subprocess.run([*cli, 'shell', 'dumpsys window windows'], capture_output=True, text=True)
            if state.returncode == 0:
                windows = json.loads(state.stdout).get('stdout', '')
                # Closing windows can still report HAS_DRAWN while the next APK
                # is installing. Require a fresh token and reject EXITING windows.
                if ready_vulkan_window(windows, previous_tokens):
                    (out / 'vulkan-ready-window.txt').write_text(windows)
                    overlay = json.loads(subprocess.check_output(
                        [*cli, 'shell', 'cmd overlay dump com.android.shell:glidepod_displays'],
                        text=True))['stdout']
                    (out / 'overlay-before.txt').write_text(overlay)
                    break
            if time.monotonic() >= deadline:
                raise TimeoutError('Vulkan activity did not draw a window')
            time.sleep(.2)
        second = pool.submit(run, 'gles')
        a, b = first.result(), second.result()
    vk, gl = a[0], b[0]
    final_overlay = json.loads(subprocess.check_output(
        [*cli, 'shell', 'cmd overlay dump com.android.shell:glidepod_displays'],
        text=True))['stdout']
    (out / 'overlay-after.txt').write_text(final_overlay)
    assert overlay == final_overlay, 'Opening another app changed the display overlay'
    assert a[1] < b[2] and b[1] < a[2]
    assert vk['epoch'] == gl['epoch'] and vk['renderer'] == gl['renderer']
    assert vk['display_id'] != gl['display_id'] and min(vk['display_id'], gl['display_id']) > 0
    assert min(vk['host_frames'], gl['host_frames']) > 20
    assert vk['guest']['vk_success'] and gl['guest']['gles3_probe']['correctness']['passed']
    report = {'passed': True, 'renderer': vk['renderer'], 'epoch': vk['epoch'],
              'display_ids': [vk['display_id'], gl['display_id']],
              'host_frames': [vk['host_frames'], gl['host_frames']],
              'display_overlay_unchanged': True,
              'invocation_overlap_seconds': min(a[2], b[2]) - max(a[1], b[1]),
              'native_macos_windows_visually_verified': False,
              'performance_comparison': False}
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
