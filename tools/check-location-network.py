#!/usr/bin/env python3
"""Location acceptance and fail-closed bridge checks in a disposable registered VM."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import http.server
import threading
import hashlib
import shlex

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', required=True, type=Path)
    args = parser.parse_args()
    store = args.data_dir.resolve()
    assert store.is_relative_to(ROOT/'.cache'), 'Use a disposable project-local VM'
    cli = [str(ROOT/'target/debug/glidepodctl'), '--data-dir', str(store)]
    env = dict(os.environ, GLIDEPOD_ANDROID_RESOURCES=str(ROOT/'target/android'))
    out = ROOT/'evidence/location-network'; out.mkdir(parents=True, exist_ok=True)
    child = None
    checks = []
    def run(*parts): return subprocess.check_output(cli+list(parts), env=env, text=True, timeout=120)
    def record(name, data): (out/(name+'.json')).write_text(json.dumps(data, indent=2))
    def state(): return json.loads(run('--json','status'))['runtime']
    def start(label):
        nonlocal child
        child = subprocess.Popen(cli+['start'], env=env, stdin=subprocess.DEVNULL,
                                 stdout=(out/(label+'-boot.log')).open('w'), stderr=subprocess.STDOUT)
        deadline = time.monotonic()+180
        while True:
            current = state()
            if current['vm']['state']=='running': return current
            if current['vm']['state']=='failed' and child.poll() is not None: return current
            assert child.poll() is None, (out/(label+'-boot.log')).read_text()
            assert time.monotonic()<deadline, current
            time.sleep(.5)
    def stop():
        nonlocal child
        if state()['vm']['state'] in ('running','starting','stopping'): run('stop')
        if child: child.wait(timeout=45); child=None
    def shell(command): return run('shell', command)
    def fix(): return json.loads(shell('su 0 cat /data/user/0/dev.glidepod.fixture/files/location.json'))
    def launch():
        shell('input keyevent KEYCODE_WAKEUP; wm dismiss-keyguard')
        shell('am force-stop dev.glidepod.fixture')
        shell('am start -W -n dev.glidepod.fixture/.LocationActivity')
    def wait_point(latitude, longitude):
        deadline=time.monotonic()+30
        while True:
            value=fix()
            if value.get('state')=='received' and abs(value['latitude']-latitude)<.00001 and abs(value['longitude']-longitude)<.00001: return value
            assert time.monotonic()<deadline, value
            time.sleep(.5)
    def shared_tcp():
        deadline=time.monotonic()+45
        while ' inet ' not in shell('ip -o -4 addr show dev wlan0'):
            assert time.monotonic()<deadline, run('--json','network-mode')
            time.sleep(.5)
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                data=b'glidepod-shared-loopback-ok\n'
                self.send_response(200);self.send_header('Content-Length',str(len(data)))
                self.end_headers();self.wfile.write(data)
            def log_message(self,*args): pass
        server=http.server.HTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        try:
            output=shell(f"printf 'GET / HTTP/1.0\\r\\nHost: 10.0.2.2\\r\\n\\r\\n' | toybox nc -w 5 10.0.2.2 {server.server_address[1]}")
            assert 'glidepod-shared-loopback-ok' in output, output
        finally:
            server.shutdown();server.server_close()
    record('report', {'passed':False,'state':'running'})
    try:
        stop()
        run('network-mode','shared')
        run('location','virtual','--latitude','1.3','--longitude','103.8')
        current=start('shared-location')
        assert current['vm']['state']=='running', current
        assert current['effective_network']['mode']=='shared', current
        shared_tcp()
        checks.append('Shared delivers Android TCP payloads to a controlled Mac loopback endpoint')
        fixture=ROOT/'target/android/glidepod-fixture.apk'
        installed=shell('pm path dev.glidepod.fixture || true').strip().removeprefix('package:')
        same=installed.startswith('/data/app/') and shell('su 0 sha256sum '+shlex.quote(installed)).split()[0]==hashlib.sha256(fixture.read_bytes()).hexdigest()
        if not same: run('install',str(fixture))
        for permission in ('ACCESS_FINE_LOCATION','ACCESS_COARSE_LOCATION'):
            shell('pm revoke dev.glidepod.fixture android.permission.'+permission)
        launch(); assert fix()['state']=='permission_denied'
        checks.append('location permission is enforced for an ordinary Android app')
        for permission in ('ACCESS_COARSE_LOCATION','ACCESS_FINE_LOCATION'):
            shell('pm grant dev.glidepod.fixture android.permission.'+permission)
        launch(); record('virtual-first',wait_point(1.3,103.8))
        checks.append('GPS fixture receives virtual coordinates after boot')
        run('location','virtual','--latitude','-33.8','--longitude','151.2')
        record('virtual-updated',wait_point(-33.8,151.2))
        checks.append('live coordinate update reaches Android without a reboot')
        rejected=subprocess.run(cli+['location','virtual','--latitude','91','--longitude','0'],env=env,capture_output=True,text=True)
        assert rejected.returncode!=0
        assert json.loads(run('--json','status'))['configuration']['location']['point']['latitude']==-33.8
        checks.append('invalid coordinates do not change the saved configuration')
        run('location','off')
        deadline=time.monotonic()+15
        while shell('cmd location is-location-enabled --user 0').strip()!='false':
            assert time.monotonic()<deadline
            time.sleep(.5)
        before=fix();time.sleep(5);assert fix()==before
        checks.append('Off disables Android location and stops new fixes')
        run('location','host')
        time.sleep(4)
        host=json.loads(run('--json','location'));record('unpackaged-host-status',host)
        assert not host['delivering'], host
        assert shell('cmd location is-location-enabled --user 0').strip()=='false'
        checks.append('unavailable Mac source never falls back to virtual coordinates')
        run('location','off')
        shell('cmd wifi set-wifi-enabled disabled')
        stop();current=start('shared-reconnect')
        assert current['vm']['state']=='running', current
        shared_tcp()
        checks.append('Shared reconnects at boot after Android Wi-Fi was disabled')
        stop()
        interfaces=json.loads(run('--json','network-mode'))['bridge_interfaces']
        if interfaces:
            run('network-mode','bridged','--interface',interfaces[0])
            bridge=start('bridge')
            record('bridge-start',bridge)
            if bridge['vm']['state']=='failed':
                assert 'vmnet' in bridge['message'] or 'bridge' in bridge['message'].lower(), bridge
                assert not bridge.get('effective_network'), bridge
                checks.append('bridge failure is explicit and never silently switches to Shared')
            else:
                assert bridge['effective_network']['mode']=='bridged'
                checks.append('bridge booted; LAN delivery still requires separate endpoint acceptance')
            stop()
        run('network-mode','shared')
        record('report',{'passed':True,'checks':checks,'actual_mac_location_verified':False,'bridged_lan_delivery_verified':False})
        print('PASS',json.dumps(checks),flush=True)
    except BaseException as error:
        record('report',{'passed':False,'checks':checks,'error':str(error)})
        raise
    finally:
        stop()
        run('location','off')
        run('network-mode','shared')

if __name__=='__main__': main()
