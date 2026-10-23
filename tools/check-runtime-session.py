#!/usr/bin/env python3
"""Exercise the real experimental VM in an already registered, disposable data directory."""
import argparse
import datetime
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/debug/glidepodctl')
    args = parser.parse_args()
    data = args.data_dir.resolve()
    evidence = ROOT / 'evidence/g1/runtime-session'
    evidence.mkdir(parents=True, exist_ok=True)
    prefix = [str(args.cli.resolve()), '--data-dir', str(data), '--json']
    def command(*parts, check=True):
        return subprocess.run([*prefix, *parts], capture_output=True, text=True, check=check)
    def status():
        return json.loads(command('status').stdout)['runtime']
    def await_state(wanted, seconds=145):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            s = status()
            if s['vm']['state'] in wanted:
                return s
            time.sleep(.3)
        raise AssertionError(f'Timed out waiting for {wanted}: {s}')
    def gone(pid, seconds=35):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return True
            time.sleep(.3)
        raise AssertionError(f'Engine process {pid} remained alive')
    def start(name):
        log = (evidence / f'{name}.jsonl').open('w')
        process = subprocess.Popen([*prefix, 'start'], stdout=log, stderr=subprocess.STDOUT)
        time.sleep(.15)
        children = subprocess.run(['pgrep', '-P', str(process.pid)], capture_output=True, text=True)
        pids = [int(p) for p in children.stdout.split()]
        assert len(pids) == 1, f'Expected the owned CLI child of RTK: {pids}'
        return process, log, pids[0]
    assert not status()['vm']['state'] in ('running', 'starting', 'stopping'), 'Stop existing test sessions first'
    checks = []
    command('profile', 'personal')
    assert command('start', check=False).returncode != 0
    command('profile', 'developer')
    command('configure', '--renderer', 'metal')
    assert command('start', check=False).returncode != 0
    command('configure', '--renderer', 'software')
    checks.append('Personal profile and unsupported Metal requests rejected')
    for name, action in [('graceful-1','stop'), ('graceful-2','stop'), ('owner-exit','owner'), ('engine-failure','engine')]:
        process, log, owner_pid = start(name)
        try:
            running = await_state({'running', 'failed'})
            assert running['vm']['state'] == 'running', running
            assert running['effective_root'] and running['effective_renderer'] == 'software'
            assert running['profile'] == 'developer'
            for _ in range(20):
                with socket.socket(socket.AF_UNIX) as disconnected:
                    disconnected.connect(str(data / 'session.sock'))
            assert command('profile', 'personal', check=False).returncode != 0
            duplicate = command('start', check=False)
            assert duplicate.returncode != 0, 'A second supervisor was accepted'
            pid = running['engine_pid']
            (evidence / f'{name}-running.json').write_text(json.dumps(running, indent=2)+'\n')
            if action == 'stop': command('stop')
            elif action == 'owner': os.kill(owner_pid, signal.SIGKILL)
            else: os.kill(pid, signal.SIGKILL)
            stopped = await_state({'stopped','failed'}, 45)
            assert stopped['vm']['state'] == ('failed' if action == 'engine' else 'stopped'), stopped
            gone(pid)
            process.wait(timeout=10)
            checks.append(f'{name}: readiness verified; engine exited and was reaped; state={stopped["vm"]["state"]}')
        finally:
            if process.poll() is None:
                os.kill(owner_pid, signal.SIGTERM)
                process.wait(timeout=30)
            log.close()
    process, log, owner_pid = start('cancel-start')
    try:
        time.sleep(.5)
        os.kill(owner_pid, signal.SIGKILL)
        process.wait(timeout=5)
        s = await_state({'stopped','failed'}, 45)
        assert not s['effective_root']
        checks.append('Owner exit during startup cancelled the session')
    finally:
        log.close()
    report = {'recorded_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'cli':str(args.cli.resolve()), 'data_dir':str(data), 'checks': checks, 'android_gpu_verified':False, 'engine_bundled':False}
    (evidence/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))

if __name__ == '__main__': main()
