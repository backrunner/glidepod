#!/usr/bin/env python3
"""Measure a running disposable VM, without changing its driver or engine.

Runs are sequential and uncapped by presentation. A failed GLES feature check
still rejects the candidate, even when diagnostic workload timings are retained.
"""
import argparse
import ctypes
import datetime
import json
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/debug/glidepodctl')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--api', choices=['gles', 'vulkan'], default='gles')
    parser.add_argument('--case', help='Run only one named case from the selected API matrix')
    parser.add_argument('--seconds', type=int, default=30)
    parser.add_argument('--repeats', type=int, default=2)
    parser.add_argument('--allow-incompatible-probe', action='store_true')
    args = parser.parse_args()
    store = args.data_dir.resolve()
    if not store.is_relative_to(ROOT / '.cache'):
        parser.error('Use a disposable project-local store')
    if not 1 <= args.seconds <= 120 or not 1 <= args.repeats <= 10:
        parser.error('Seconds must be 1–120 and repeats 1–10')
    cases = ([('draw-calls', 512), ('fragment', 2048), ('texture', 2048)]
             if args.api == 'gles' else [('indexed-256', 512), ('indexed-64', 2048), ('indexed-4096', 2048)])
    if args.case:
        cases = [case for case in cases if case[0] == args.case]
        if not cases:
            parser.error('The requested case is not in this API matrix')
    args.output.mkdir(parents=True, exist_ok=False)
    cli = [str(args.cli.resolve()), '--data-dir', str(store), '--json']
    runtime = json.loads(subprocess.check_output(cli + ['status'], text=True))['runtime']
    pid = runtime['engine_pid']
    if not pid or runtime['vm']['state'] != 'running':
        raise RuntimeError('Start the intended VM and renderer before measuring')
    proc = ctypes.CDLL('/usr/lib/libproc.dylib')
    timebase = (ctypes.c_uint32 * 2)()
    if ctypes.CDLL('/usr/lib/libSystem.B.dylib').mach_timebase_info(ctypes.byref(timebase)) != 0:
        raise RuntimeError('Cannot read Mach timebase')

    def cpu_seconds():
        usage = (ctypes.c_uint64 * 20)()
        if proc.proc_pid_rusage(pid, 2, ctypes.byref(usage)) != 0:
            raise RuntimeError('Cannot measure VMM resource usage')
        return (usage[2] + usage[3]) * timebase[0] / timebase[1] / 1e9

    records = []
    latest = store / 'developer/stress' / ('latest.json' if args.api == 'gles' else 'latest-vulkan.json')
    for repeat in range(args.repeats):
        # Reverse the workload order on alternate passes.
        for mode, size in (reversed(cases) if repeat % 2 else cases):
            name = f'{repeat + 1}-{mode}'
            previous = latest.read_bytes() if latest.exists() else b''
            start, cpu_start = time.monotonic(), cpu_seconds()
            workload = (['graphics-stress', '--workload', mode] if args.api == 'gles'
                        else ['vulkan-stress', '--objects', mode.split('-')[1]])
            command = cli + workload + ['--width', str(size), '--height', str(size),
                                         '--seconds', str(args.seconds)]
            result = subprocess.run(command, capture_output=True, text=True,
                                    timeout=args.seconds + 180)
            elapsed, cpu = time.monotonic() - start, cpu_seconds() - cpu_start
            (args.output / f'{name}.stderr').write_text(result.stderr)
            if not latest.exists() or latest.read_bytes() == previous:
                raise RuntimeError(f'{name}: no fresh diagnostic report: {result.stderr}')
            report = json.loads(latest.read_text())
            (args.output / f'{name}.json').write_text(json.dumps(report, indent=2) + '\n')
            guest = report['guest']
            if (report['runtime']['vm'] != runtime['vm']
                    or guest.get('width') != size or guest.get('height') != size
                    or guest.get('seconds_requested') != args.seconds
                    or guest.get('measured_ms', 0) < args.seconds * 1000
                    or guest.get('frames', 0) <= 0):
                raise RuntimeError(f'{name}: invalid workload, duration or session')
            if args.api == 'gles':
                if (guest.get('workload') != mode.replace('-', '_') or guest.get('gl_error') != 0
                        or guest.get('reference_pixel_passed') is not True):
                    raise RuntimeError(f'{name}: GLES workload failed')
                correct = guest.get('gles3_correctness', {}).get('passed') is True
            else:
                if guest.get('objects') != int(mode.split('-')[1]) or guest.get('validation_enabled') is not False:
                    raise RuntimeError(f'{name}: wrong Vulkan workload or validation overhead')
                correct = guest.get('vk_success') is True and guest.get('validation_errors') == 0
            record = {'file': f'{name}.json', 'cli_exit_code': result.returncode,
                      'correctness_passed': correct, 'wall_seconds': elapsed,
                      'vmm_cpu_percent_including_setup': cpu / elapsed * 100,
                      'completed_frames_per_second': guest['completed_frames_per_second'],
                      'completion_ms': guest['completion_ms']}
            records.append(record)
            summary = {'recorded_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                       'label': args.label, 'api': args.api, 'runtime': runtime,
                       'passed': all(r['correctness_passed'] and r['cli_exit_code'] == 0 for r in records),
                       'native_presentation_measured': False,
                       'cpu_tick_to_ns': timebase[0] / timebase[1], 'records': records}
            (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
            print(json.dumps({'case': name, **record}), flush=True)
            if (result.returncode or not correct) and not args.allow_incompatible_probe:
                raise RuntimeError(f'{name}: candidate failed correctness: {result.stderr}')
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
