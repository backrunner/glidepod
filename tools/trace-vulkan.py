#!/usr/bin/env python3
"""Capture guest scheduling and diagnostic content arrival for Vulkan recreation.

Only disposable stores under .cache are accepted. Host timestamps describe
observations of the copied scanout, not WindowServer/physical display timing.
"""
import argparse
import base64
import hashlib
import json
import mmap
from pathlib import Path
import shlex
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
CONFIG = '''buffers { size_kb: 65536 fill_policy: RING_BUFFER }
duration_ms: 30000
data_sources { config { name: "linux.ftrace"
  ftrace_config {
    symbolize_ksyms: true
    ftrace_events: "sched/sched_switch"
    ftrace_events: "sched/sched_waking"
    ftrace_events: "sched/sched_migrate_task"
    ftrace_events: "workqueue/workqueue_execute_start"
    ftrace_events: "workqueue/workqueue_execute_end"
    ftrace_events: "irq/irq_handler_entry"
    ftrace_events: "irq/irq_handler_exit"
    atrace_categories: "gfx"
    atrace_categories: "view"
    atrace_categories: "wm"
    atrace_categories: "am"
    atrace_categories: "binder_driver"
    atrace_apps: "dev.glidepod.vulkan"
  }
}}
data_sources { config { name: "linux.process_stats"
  process_stats_config { scan_all_processes_on_start: true }
}}
data_sources { config { name: "android.surfaceflinger.frametimeline" }}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/debug/glidepodctl')
    parser.add_argument('--no-recreate', action='store_true')
    parser.add_argument('--samples', type=int, default=180, choices=range(60, 601), metavar='60..600')
    args = parser.parse_args()
    data, out = args.data_dir.resolve(), args.output.resolve()
    assert data.is_relative_to(ROOT / '.cache')
    assert out.is_relative_to(ROOT / 'evidence') and not out.exists()
    out.mkdir(parents=True)
    cli = [str(args.cli.resolve()), '--data-dir', str(data), '--json']

    def shell(command):
        r = subprocess.run([*cli, 'shell', command], capture_output=True, text=True, timeout=40)
        if r.returncode:
            raise RuntimeError(r.stderr)
        return json.loads(r.stdout)['stdout']

    status = json.loads(subprocess.check_output([*cli, 'status']))
    assert status['runtime']['vm']['state'] == 'running'
    frames = Path(status['runtime']['endpoints']['frames'])
    token = str(time.time_ns())
    guest_config = f'/data/local/tmp/glidepod-vulkan-{token}.cfg'
    guest_trace = f'/data/misc/perfetto-traces/glidepod-vulkan-{token}.pftrace'
    shell(f'printf %s {shlex.quote(CONFIG)} > {shlex.quote(guest_config)}')
    (out / 'config.pbtxt').write_text(CONFIG)
    trace_pid = shell(f'perfetto --background-wait --txt -c {guest_config} -o {guest_trace}').strip()
    assert trace_pid.isdigit(), trace_pid
    (out / 'trace-pid.txt').write_text(trace_pid + '\n')
    (out / 'trace-session.json').write_text(json.dumps({'pid': int(trace_pid), 'config': guest_config,
                                                       'trace': guest_trace, 'runtime': status['runtime']}, indent=2) + '\n')
    maps, previous, observations, polls = {}, {}, [], []
    proc = None
    try:
        with (out / 'benchmark.json').open('w') as result, (out / 'benchmark.log').open('w') as log:
            command = [*cli, 'benchmark', '--api', 'vulkan', '--trace-stages', '--samples', str(args.samples)]
            if args.no_recreate:
                command.append('--no-recreate')
            proc = subprocess.Popen(command, stdout=result, stderr=log)
            deadline = time.monotonic() + 60
            while proc.poll() is None:
                now = time.monotonic_ns()
                polls.append(now)
                if time.monotonic() >= deadline:
                    raise TimeoutError('Diagnostic benchmark exceeded 60 seconds')
                for path in frames.glob('scanout-*.frames'):
                    if path.name.startswith('scanout-0-'):
                        continue
                    if path not in maps:
                        try:
                            with path.open('rb') as f:
                                maps[path] = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
                        except (FileNotFoundError, ValueError):
                            continue
                    mm = maps[path]
                    header = mm[:56]
                    if header[:4] != b'AVOF':
                        continue
                    version, generation, width, height, stride, fmt, buffers, offset, size, index = struct.unpack_from('<10I', header, 4)
                    seq = struct.unpack_from('<Q', header, 48)[0]
                    if seq == previous.get(path) or not seq or version != 1:
                        continue
                    assert fmt in (0, 1) and index < buffers and stride >= width * 4
                    assert offset + buffers * size <= len(mm) and size >= stride * height
                    if index != seq % buffers:
                        continue  # Writer published the new slot but not its sequence yet.
                    # Search the left edge for the 8-pixel diagnostic stamp;
                    # account for possible Android window insets without guessing.
                    marker, marker_y = None, None
                    base = offset + index * size + 4 * 4
                    for y in range(min(height, 256)):
                        pixel = mm[base + y * stride:base + y * stride + 4]
                        r, g, b, a = pixel if fmt == 1 else (pixel[2], pixel[1], pixel[0], pixel[3])
                        if b == 167 and a == 255:
                            marker, marker_y = r + (g << 8), y
                            break
                    if mm[40:44] != header[40:44] or mm[48:56] != header[48:56]:
                        continue
                    previous[path] = seq
                    observations.append({'host_monotonic_ns': now, 'path': path.name, 'sequence': seq,
                                         'frame': marker, 'marker_y': marker_y,
                                         'width': width, 'height': height, 'format': fmt})
                time.sleep(.002)
            if proc.returncode:
                raise RuntimeError(f'Benchmark failed: see {out / "benchmark.log"}')
    finally:
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            shell('am force-stop dev.glidepod.vulkan')
        for mm in maps.values():
            mm.close()
        (out / 'host-observations.json').write_text(json.dumps(observations, indent=2) + '\n')
        (out / 'host-polls.json').write_text(json.dumps(polls) + '\n')
        # Stop only the Perfetto process created above, never a pre-existing trace.
        # The process can exit between testing /proc and opening cmdline.
        probe = f'cat /proc/{trace_pid}/cmdline 2>/dev/null || true'
        owner = shell(probe).replace('\0', ' ')
        if guest_trace in owner:
            shell(f'kill -TERM {trace_pid}')
            # Wait for the trace writer to close before reading the file.
            for _ in range(50):
                if not shell(probe).strip('\0'):
                    break
                time.sleep(.1)
            else:
                raise TimeoutError('Perfetto did not finish writing its trace')
        length = int(shell(f'stat -c %s {guest_trace}').strip())
        assert 0 < length < 128 * 1024 * 1024
        with (out / 'guest.pftrace').open('wb') as trace:
            block = 512 * 1024
            for skip in range((length + block - 1) // block):
                encoded = shell(f'dd if={guest_trace} bs={block} skip={skip} count=1 2>/dev/null | base64')
                trace.write(base64.b64decode(encoded))
        assert (out / 'guest.pftrace').stat().st_size == length
        shell(f'rm {guest_config} {guest_trace}')
    report = {'epoch': status['runtime']['vm']['epoch'], 'renderer': status['runtime']['effective_renderer'],
              'fingerprint': status['runtime']['fingerprint'],
              'cli_sha256': hashlib.sha256(args.cli.read_bytes()).hexdigest(),
              'installed_apk_sha256': shell('p=$(pm path dev.glidepod.vulkan); sha256sum "${p#package:}"').split()[0],
              'host_observations': len(observations), 'content_markers': sum(v['frame'] is not None for v in observations),
              'poll_interval_ms': 2, 'trace_bytes': length,
              'scope': 'Guest trace and copied host scanout observations; no native/physical presentation measurement'}
    (out / 'capture.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
