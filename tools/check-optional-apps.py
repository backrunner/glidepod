#!/usr/bin/env python3
"""Destructive lifecycle acceptance in a disposable, already GMS-equipped test store."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', required=True, type=Path)
    a = parser.parse_args()
    store = a.data_dir.resolve()
    assert store.is_relative_to(ROOT / '.cache'), 'Use a disposable test store'
    cli = [str(ROOT / 'target/debug/glidepodctl'), '--data-dir', str(store)]
    env = dict(os.environ, GLIDEPOD_ANDROID_RESOURCES=str(ROOT / 'target/android'))
    out = ROOT / 'evidence/optional-system-apps'
    out.mkdir(parents=True, exist_ok=True)
    checks = []
    child = None
    def run(*parts):
        return subprocess.check_output(cli + list(parts), env=env, text=True, timeout=150)
    def record(name, value):
        (out / (name + '.json')).write_text(json.dumps(value, indent=2))
    def wait_stopped():
        deadline = time.monotonic() + 45
        while json.loads(run('--json', 'status'))['runtime']['vm']['state'] not in ['stopped','failed']:
            assert time.monotonic() < deadline
            time.sleep(.2)
    def stop():
        nonlocal child
        status = json.loads(run('--json','status'))['runtime']['vm']['state']
        if status in ['running','starting','stopping']:
            run('stop')
            wait_stopped()
        if child:
            child.wait(timeout=10)
            child = None
    def start(label):
        nonlocal child
        child = subprocess.Popen(cli + ['start'], env=env, stdin=subprocess.DEVNULL,
                                 stdout=(out / (label + '-boot.log')).open('w'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 180
        while True:
            status = json.loads(run('--json','status'))['runtime']
            if status['vm']['state'] == 'running': break
            assert child.poll() is None, (out / (label + '-boot.log')).read_text()
            assert time.monotonic() < deadline, status
            time.sleep(.5)
        print(label, 'booted', flush=True)
    def system_apps(label):
        r = json.loads(run('shell','su 0 env CLASSPATH=/data/local/tmp/glidepod/system-apps-bridge.jar app_process / dev.glidepod.bridge.SystemApps status'))
        record(label, r)
        return r['apps']
    def gms(label, enabled):
        r = json.loads(run('--json','gms'))
        record(label, r)
        assert all(p['installed'] and p['enabled'] == enabled for p in r['inventory']['packages']), r
        assert 'preserved-google-data' in run('shell','su 0 cat /data/user/0/com.google.android.gms/files/glidepod-toggle-check')
        assert 'preserved-clock-data' in run('shell','su 0 cat /data/user/0/com.android.deskclock/files/glidepod-trim-check')
        checks.append(label)
    def manage(action, cancel_at=None):
        nonlocal child
        stop()
        lines = []
        proc = subprocess.Popen(cli + ['google-services', action], env=env,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        cancelled = False
        for line in proc.stdout:
            lines.append(line)
            print(action, line.strip(), flush=True)
            if cancel_at and cancel_at in line and not cancelled:
                proc.send_signal(signal.SIGINT)
                cancelled = True
        result = proc.wait(timeout=20)
        (out / (action + ('-cancel' if cancel_at else '') + '.log')).write_text(''.join(lines))
        assert (result != 0 and cancelled) if cancel_at else result == 0, ''.join(lines)
        assert not (store / 'google-apps-transaction.json').exists()
        if not cancel_at and action in ('disable','remove'):
            cache = store/'developer/apps.json'
            assert not cache.exists() or all(p['package_id']!='com.android.vending' for p in json.loads(cache.read_text()))
    record('report', {'passed':False,'state':'running','checks':[]})
    try:
        stop()
        run('configure','--builtin-apps','full')
        start('baseline')
        # Prior file-regression fixtures can leave retrying downloads. Those correctly
        # retain the VM even with GMS off; remove only this disposable fixture's queue.
        fixture_downloads = "--uri content://downloads/all_downloads --where \"notificationpackage='dev.glidepod.filefixture'\""
        (out/'prior-fixture-downloads.log').write_text(run('shell', 'su 0 content query ' + fixture_downloads + ' --projection _id:status:uri:notificationpackage'))
        run('shell', 'su 0 content delete ' + fixture_downloads)
        baseline = system_apps('baseline')
        run('apps') # Seed the cached Play Store entry before testing invalidation.
        expected = {p['package_id']:p['enabled'] for p in baseline if p['installed'] and p['system']}
        assert len(expected) == 18, 'This acceptance fixture requires the pinned Android 17 image'
        assert all(not p['managed'] for p in baseline)
        assert all(p['installed'] for p in json.loads(run('--json','gms'))['inventory']['packages']), 'Install GMS in this disposable store first'
        for package, filename, contents in [
            ('com.google.android.gms','glidepod-toggle-check','preserved-google-data'),
            ('com.android.deskclock','glidepod-trim-check','preserved-clock-data'),
        ]:
            run('shell', f"su 0 sh -c 'mkdir -p /data/user/0/{package}/files; echo {contents} > /data/user/0/{package}/files/{filename}'")
        stop()
        run('configure','--builtin-apps','minimal')
        start('trimmed')
        assert all(not p['enabled'] and p['managed'] for p in system_apps('trimmed'))
        stop()
        run('configure','--builtin-apps','full')
        start('restored')
        apps = system_apps('restored')
        assert {p['package_id']:p['enabled'] for p in apps} == expected
        assert all(not p['managed'] for p in apps)
        checks.append('trim and restore preserve original enabled and disabled states')
        run('configure','--builtin-apps','minimal')
        manage('disable')
        start('gms-off')
        gms('disabled-data-preserved',False)
        assert all(not p['enabled'] for p in system_apps('minimal-reapplied'))
        busy = json.loads(run('shell','su 0 env CLASSPATH=/data/local/tmp/glidepod/system-apps-bridge.jar app_process / dev.glidepod.bridge.Background'))
        record('disabled-background', busy)
        assert not busy['busy'], busy
        running = run('shell','ps -A')
        assert not any(line.split()[-1].startswith(('com.google.android.gms','com.android.vending')) for line in running.splitlines() if line.split())
        checks.append('disabled GMS has no processes and does not pin idle runtime')
        manage('enable', 'Turning on Google services')
        start('cancelled-enable')
        gms('cancelled-enable-restored-original',False)
        manage('install')
        start('gms-on')
        gms('install-turns-existing-services-back-on',True)
        manage('disable')
        manage('remove')
        start('removed')
        r=json.loads(run('--json','gms'));record('removed',r)
        assert all(not p['installed'] for p in r['inventory']['packages'])
        assert 'preserved-clock-data' in run('shell','su 0 cat /data/user/0/com.android.deskclock/files/glidepod-trim-check')
        checks.append('removal from off state preserves unrelated app data')
        stop()
        archive = store/'google-apps-cache/package.zip'
        archive.parent.mkdir(exist_ok=True)
        shutil.copyfile(ROOT/'.cache/google-apps/MindTheGapps-17.0.0-arm64-20261022_132445.zip',archive)
        manage('install')
        start('reinstalled')
        r=json.loads(run('--json','gms'));record('reinstalled',r)
        assert all(p['installed'] and p['enabled'] for p in r['inventory']['packages'])
        assert all(not p['enabled'] for p in system_apps('reinstalled-minimal'))
        assert 'preserved-clock-data' in run('shell','su 0 cat /data/user/0/com.android.deskclock/files/glidepod-trim-check')
        checks.append('clean reinstall returns enabled GMS and retains minimal defaults')
        record('report', {'passed':True,'checks':checks,'account_login_tested':False,'remote_fcm_tested':False})
        print('PASS',flush=True)
    except BaseException as error:
        record('report', {'passed':False,'state':'failed','checks':checks,'error':str(error)})
        raise
    finally:
        stop()

if __name__ == '__main__': main()
