#!/usr/bin/env python3
"""Controlled diagnostics in a disposable guest, never a production cache-flushing policy."""
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
    p.add_argument('--label', required=True)
    p.add_argument('--report-order', type=int, choices=range(2, 11), default=4)
    p.add_argument('--settle-seconds', type=int, default=45)
    p.add_argument('--experiments', action='store_true', help='Also inflate 1 GiB, compact memory and drop caches; use >=4 GiB RAM')
    a = p.parse_args()
    assert 0 <= a.settle_seconds <= 300
    assert a.data_dir.resolve().is_relative_to(ROOT / '.cache')
    assert a.label.replace('-', '').isalnum()
    cli = [str(ROOT / 'target/debug/glidepodctl'), '--data-dir', str(a.data_dir.resolve())]
    def run(*parts):
        return subprocess.check_output(cli + list(parts), text=True, timeout=120)
    deadline = time.monotonic() + 120
    while True:
        runtime = json.loads(run('--json', 'status'))['runtime']
        if runtime['vm']['state'] == 'running': break
        assert time.monotonic() < deadline, runtime
        time.sleep(1)
    if a.experiments:
        assert runtime['resources']['memory_mib'] >= 4096, 'Experiments require at least 4 GiB guest RAM'
    out = ROOT / 'evidence/memory-audit' / a.label
    out.mkdir(parents=True, exist_ok=True)
    records = []
    def ctl(command):
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(10)
            s.connect(runtime['endpoints']['control'])
            s.sendall((command + '\n').encode())
            data = b''
            while not data.endswith(b'\n'):
                part = s.recv(4096)
                assert part, 'Control disconnected'
                data += part
            return data.decode().strip()
    def snapshot(stage):
        reply = ctl('host-memory')
        if reply.startswith('{'):
            host = json.loads(reply)
        else:
            info = (ctypes.c_uint64 * 20)()
            assert ctypes.CDLL('/usr/lib/libproc.dylib').proc_pid_rusage(runtime['engine_pid'], 2, ctypes.byref(info)) == 0
            host = {'footprint': info[9], 'resident': info[8]}
        guest = run('shell', 'cat /proc/meminfo /proc/buddyinfo /proc/pressure/memory; pidof system_server; cat /proc/uptime')
        (out / f'{stage}-guest.txt').write_text(guest)
        residency = ctl('memory-residency')
        residency = json.loads(residency) if residency.startswith('{') else None
        record = {'residency': residency, 'stage': stage, 'time': time.time(), 'host': host, 'balloon': json.loads(ctl('memory-status')), 'guest': guest}
        records.append(record)
        (out / 'measurements.json').write_text(json.dumps({'runtime': runtime, 'records': records}, indent=2) + '\n')
        print(a.label, stage, {k:round(v/1048576,1) for k,v in host.items()}, flush=True)
        return record
    time.sleep(10)
    snapshot('baseline')
    run('shell', f"su 0 sh -c 'test -d /sys/module/virtio_balloon || insmod /system_dlkm/lib/modules/virtio_balloon.ko; echo {a.report_order} > /sys/module/page_reporting/parameters/page_reporting_order'")
    time.sleep(a.settle_seconds)
    snapshot('reporting')
    (out / 'android-memory.txt').write_text(run('shell', 'dumpsys meminfo; ps -A | grep -E "system_server|gms|gsf|vending"'))
    if not a.experiments:
        return
    run('shell', 'su 0 dd if=/dev/urandom of=/dev/glidepod-audit-keeper bs=1048576 count=32')
    keeper = run('shell', 'su 0 sha256sum /dev/glidepod-audit-keeper').split()[0]
    try:
        assert ctl('memory 1024') == 'ok'
        end = time.monotonic() + 30
        while json.loads(ctl('memory-status'))['actual_mib'] < 1024:
            assert time.monotonic() < end
            time.sleep(.2)
        time.sleep(5)
        snapshot('balloon1024')
        run('shell', "su 0 sh -c 'echo 1 > /proc/sys/vm/compact_memory'")
        time.sleep(8)
        snapshot('compact')
        run('shell', "su 0 sh -c 'sync; echo 3 > /proc/sys/vm/drop_caches'")
        time.sleep(8)
        snapshot('drop_clean_caches')
        run('shell', 'input keyevent 223')
        time.sleep(5)
        snapshot('screen_off')
        assert run('shell', 'su 0 sha256sum /dev/glidepod-audit-keeper').split()[0] == keeper
        (out / 'checks.json').write_text(json.dumps({'live_data_preserved': True, 'boot_completed': run('shell', 'getprop sys.boot_completed').strip()}))
    finally:
        ctl('memory 0')
        run('shell', 'input keyevent 224; su 0 rm -f /dev/glidepod-audit-keeper')

if __name__ == '__main__': main()
