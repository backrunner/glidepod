#!/usr/bin/env python3
"""Measure real macOS reclamation and guest data integrity in a disposable running VM."""
import argparse
import ctypes
import json
import socket
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/debug/glidepodctl')
    args = parser.parse_args()
    assert args.data_dir.resolve().is_relative_to(ROOT / '.cache'), 'Use a disposable project-local store'
    prefix = [str(args.cli.resolve()), '--data-dir', str(args.data_dir.resolve())]
    def cli(*parts):
        return subprocess.check_output(prefix + list(parts), text=True, timeout=120)
    status = json.loads(cli('--json', 'status'))['runtime']
    assert status['vm']['state'] == 'running'
    pid = status['engine_pid']
    def control(command):
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(10)
            s.connect(status['endpoints']['control'])
            s.sendall((command + '\n').encode())
            data = b''
            while not data.endswith(b'\n'): data += s.recv(4096)
            return data.decode().strip()
    def footprint():
        # rusage_info_v2: UUID[16], followed by 18 u64 fields. Avoid vmmap's GiB rounding.
        info = (ctypes.c_uint64 * 20)()
        assert ctypes.CDLL('/usr/lib/libproc.dylib').proc_pid_rusage(pid, 2, ctypes.byref(info)) == 0
        return info[9]
    records = []
    def record(label):
        item = {'stage': label, 'phys_footprint_bytes': footprint(), 'balloon': json.loads(control('memory-status')),
                'backing': json.loads(control('memory-residency')),
                'guest': cli('shell', 'grep -E "^(MemTotal|MemAvailable|SwapFree):" /proc/meminfo; cat /proc/pressure/memory')}
        records.append(item)
        print(label, round(item['phys_footprint_bytes']/1024**2), 'MiB', item['balloon'], flush=True)
        return item
    record('before_module')
    cli('shell', "su 0 sh -c 'test -d /sys/module/virtio_balloon || insmod /system_dlkm/lib/modules/virtio_balloon.ko'")
    cli('shell', "su 0 sh -c 'echo 4 > /sys/module/page_reporting/parameters/page_reporting_order'")
    deadline = time.monotonic() + 15
    while not json.loads(control('memory-status'))['reporting']:
        assert time.monotonic() < deadline, 'Driver did not negotiate reporting'
        time.sleep(.2)
    time.sleep(5)
    record('reporting_idle')
    cli('shell', 'su 0 dd if=/dev/urandom of=/dev/glidepod-memory-keeper bs=1048576 count=32')
    checksum = cli('shell', 'su 0 sha256sum /dev/glidepod-memory-keeper').split()[0]
    try:
        for cycle in range(3):
            cli('shell', 'su 0 dd if=/dev/urandom of=/dev/glidepod-memory-load bs=1048576 count=768')
            full = record(f'allocated_{cycle}')
            cli('shell', 'su 0 rm /dev/glidepod-memory-load')
            time.sleep(8)
            free = record(f'released_{cycle}')
            assert full['phys_footprint_bytes'] - free['phys_footprint_bytes'] > 256 * 1024**2, 'Guest free did not reduce host accounting'
            backed = lambda r: r['backing']['resident_bytes'] + r['backing']['paged_out_bytes']
            assert backed(full) - backed(free) > 256 * 1024**2, 'Guest free did not release substantial backing objects'
            assert cli('shell', 'su 0 sha256sum /dev/glidepod-memory-keeper').split()[0] == checksum, 'Live data changed during reclamation'
        assert control('memory 256') == 'ok'
        deadline = time.monotonic() + 15
        while json.loads(control('memory-status'))['actual_mib'] < 256:
            assert time.monotonic() < deadline, 'Balloon did not inflate'
            time.sleep(.2)
        record('inflated_256')
        assert control('memory 0') == 'ok'
        deadline = time.monotonic() + 15
        while json.loads(control('memory-status'))['actual_mib'] != 0:
            assert time.monotonic() < deadline, 'Balloon did not deflate'
            time.sleep(.2)
        record('deflated')
        assert cli('shell', 'su 0 sha256sum /dev/glidepod-memory-keeper').split()[0] == checksum
        assert cli('shell', 'getprop sys.boot_completed').strip() == '1'
        out = ROOT / 'evidence/adaptive-memory'
        out.mkdir(parents=True, exist_ok=True)
        (out / 'reclaim.json').write_text(json.dumps({'passed': True, 'records': records,
            'checks': ['three real guest allocation/free/reuse cycles', 'live random data checksum preserved', 'balloon inflate/deflate', 'guest remained booted'],
            'scope': 'Software renderer, disposable pinned Android guest; remote FCM not exercised'}, indent=2) + '\n')
    finally:
        control('memory 0')
        cli('shell', 'su 0 rm -f /dev/glidepod-memory-keeper /dev/glidepod-memory-load')

if __name__ == '__main__': main()
