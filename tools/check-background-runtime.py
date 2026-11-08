#!/usr/bin/env python3
"""Exercise the real detached owner in an explicitly selected disposable store."""
import argparse
import ctypes
import json
import socket
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--data-dir', required=True, type=Path)
    p.add_argument('--cli', required=True, type=Path)
    args = p.parse_args()
    store = args.data_dir.resolve()
    assert store.is_relative_to(ROOT / '.cache'), 'Use a disposable project-local store'
    cli = [str(args.cli.resolve()), '--data-dir', str(store)]
    def command(*args):
        return subprocess.check_output(cli + list(args), text=True, timeout=30)
    def status():
        return json.loads(command('--json', 'background-status'))
    def footprint(pid):
        info = (ctypes.c_uint64 * 20)()
        assert ctypes.CDLL('/usr/lib/libproc.dylib').proc_pid_rusage(pid, 2, ctypes.byref(info)) == 0
        return info[9]
    records = []
    def record(stage):
        s = status()
        record = {'stage': stage, 'time': time.time(), 'status': s, 'agent_bytes': footprint(s['pid'])}
        if s['runtime']['engine_pid']: record['engine_bytes'] = footprint(s['runtime']['engine_pid'])
        records.append(record)
        print(stage, s['reason'], record.get('engine_bytes'), flush=True)
        return s
    def wait_running():
        end = time.monotonic() + 120
        while status()['runtime']['vm']['state'] != 'running':
            assert time.monotonic() < end, 'VM did not boot'
            time.sleep(2)
    command('configure', '--runtime-mode', 'resident', '--memory-mode', 'automatic')
    command('background-start')
    wait_running()
    first = record('resident_start')
    for _ in range(100):
        with socket.socket(socket.AF_UNIX) as s: s.connect(str(store / 'background.sock'))
        assert status()['pid'] == first['pid'], 'Dropped client replaced the agent'
    # Smart mode must retain explicit intent even while an app is completely idle.
    command('background-app', 'dev.glidepod.fixture')
    command('configure', '--runtime-mode', 'automatic')
    for n in range(7):
        time.sleep(30)
        s = record(f'protected_{n}')
        assert s['runtime']['vm'] == first['runtime']['vm'], 'Protected guest stopped or restarted'
    command('background-app', 'dev.glidepod.fixture', '--disable')
    start = time.monotonic()
    while True:
        time.sleep(15)
        s = record('idle_wait')
        if s['runtime']['vm']['state'] == 'stopped': break
        assert time.monotonic() - start < 240, 'Confirmed idle guest did not stop'
    assert time.monotonic() - start >= 160, 'Guest stopped before the idle grace period'
    command('background-start')
    wait_running()
    assert record('reopened')['runtime']['vm'] != first['runtime']['vm']
    command('configure', '--runtime-mode', 'resident')
    command('stop')
    time.sleep(12)
    assert record('explicit_stop')['runtime']['vm']['state'] == 'stopped'
    command('background-quit')
    time.sleep(2)
    # Starting only the service, without Wake, must not override explicit Stop.
    helper = args.cli.resolve().parents[1] / 'Library/LoginItems/GlidePod Background.app/Contents/MacOS/glidepodctl'
    child = subprocess.Popen([str(helper), '--data-dir', str(store), 'background-worker'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        time.sleep(8)
        assert record('pause_survived_restart')['runtime']['vm']['state'] == 'stopped'
    finally:
        command('background-quit')
        child.wait(timeout=15)
    out = ROOT / 'evidence/adaptive-memory/lifecycle.json'
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({'passed': True, 'records': records}, indent=2) + '\n')

if __name__ == '__main__': main()
