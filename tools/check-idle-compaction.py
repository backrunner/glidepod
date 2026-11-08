#!/usr/bin/env python3
"""Check one automatic idle compaction in a freshly started disposable background VM."""
import argparse
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--data-dir', type=Path, required=True)
    args = parser.parse_args()
    store = args.data_dir.resolve()
    assert store.is_relative_to(ROOT / '.cache'), 'Use a disposable project-local store'
    config = json.loads((store / 'configuration.json').read_text())
    assert config['runtime_mode'] == 'resident', 'Keep the test VM alive independently of this test'
    assert config[config['active_profile']]['memory_mode'] == 'manual', 'Isolate compaction from balloon target changes'
    log = store / 'background.log'
    assert 'idle compaction completed' not in log.read_text(), 'Restart the background owner and run before its first idle minute'
    prefix = [str(ROOT / 'target/debug/glidepodctl'), '--data-dir', str(store)]
    def run(*parts):
        return subprocess.check_output(prefix + list(parts), text=True, timeout=30)
    deadline = time.monotonic() + 120
    while True:
        status = json.loads(run('--json', 'background-status'))
        if status['runtime']['vm']['state'] == 'running' and status.get('memory'):
            break
        assert time.monotonic() < deadline, status
        time.sleep(1)
    assert status['managed'], 'The background agent must own this VM'
    out = ROOT / 'evidence/memory-audit/automatic-compaction.json'
    out.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    keeper = '/dev/glidepod-compact-keeper'
    run('shell', f'su 0 dd if=/dev/urandom of={keeper} bs=1048576 count=32')
    checksum = run('shell', f'su 0 sha256sum {keeper}').split()[0]
    try:
        for i in range(6):
            started = time.monotonic()
            status = json.loads(run('--json', 'background-status'))
            row = {'stage': i, 'status_seconds': time.monotonic() - started,
                   'epoch': status['runtime']['vm']['epoch'],
                   'backing': json.loads(run('--json', 'memory')),
                   'guest': run('shell', 'cat /proc/meminfo /proc/pressure/memory; ps -A | grep -E "system_server|gms"')}
            rows.append(row)
            out.write_text(json.dumps({'records': rows}, indent=2))
            print('sample', i, row['backing'], flush=True)
            if i < 5:
                time.sleep(20)
        assert run('shell', f'su 0 sha256sum {keeper}').split()[0] == checksum
        assert len({r['epoch'] for r in rows}) == 1
        assert log.read_text().count('[memory] idle compaction completed') == 1
        assert all(r['status_seconds'] < 2 for r in rows)
        out.write_text(json.dumps({'records': rows, 'checksum_preserved': True,
                                  'same_epoch': True, 'automatic_compactions': 1,
                                  'log': log.read_text()}, indent=2))
        print('PASS', flush=True)
    finally:
        run('shell', f'su 0 rm -f {keeper}')

if __name__ == '__main__':
    main()
