#!/usr/bin/env python3
"""Relate Vulkan producer timing to content-bearing host scanout observations."""
import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    parser.add_argument('--trace-processor', type=Path, required=True)
    args = parser.parse_args()
    folder = args.capture.resolve()
    report = json.loads((folder / 'benchmark.json').read_text())
    guest = report['guest']
    rows = guest['stage_timings']
    recreating = guest['recreate_requested']
    assert guest['stage_timings_enabled'] and guest['vk_success']
    assert guest['swapchain_recreations'] == int(recreating), 'Unexpected surface lifecycle in control run'
    generation = [r for r in rows if r['generation'] == 2]
    if recreating:
        assert generation and generation[0]['generation_frame'] == 0
        anchor = generation[0]['frame']
    else:
        anchor = min((r for r in rows if r['sample'] >= 0),
                     key=lambda r: abs(r['sample'] - report['request']['samples'] // 2))['frame']
    observations = json.loads((folder / 'host-observations.json').read_text())
    markers = [r for r in observations if r['frame'] is not None]
    # Multiple active apps/displays would make a one-stream comparison ambiguous.
    paths = {r['path'] for r in markers}
    assert len(paths) == 1, paths
    unique = []
    for row in markers:
        if not unique or row['frame'] != unique[-1]['frame']:
            unique.append(row)
    near = [{'frame': b['frame'], 'delta_ms': (b['host_monotonic_ns'] - a['host_monotonic_ns']) / 1e6,
             'frame_step': b['frame'] - a['frame'], 'sequence_step': b['sequence'] - a['sequence']}
            for a, b in zip(unique, unique[1:]) if anchor - 4 <= b['frame'] <= anchor + 8]
    complete = [r['frame'] for r in near] == list(range(anchor - 4, anchor + 9))
    polls = json.loads((folder / 'host-polls.json').read_text())
    poll_deltas = [(b - a) / 1e6 for a, b in zip(polls, polls[1:])]
    window_times = [r['host_monotonic_ns'] for r in unique if anchor - 5 <= r['frame'] <= anchor + 8]
    near_poll_deltas = [(b - a) / 1e6 for a, b in zip(polls, polls[1:])
                        if window_times and a <= max(window_times) and b >= min(window_times)]
    measured_start = min(r['frame'] - r['sample'] for r in rows if r['sample'] >= 0)
    measured_gaps = []
    for a, b in zip(unique, unique[1:]):
        if a['frame'] < measured_start:
            continue
        measured_gaps.append({'before_frame': a['frame'], 'after_frame': b['frame'],
                              'delta_ms': (b['host_monotonic_ns'] - a['host_monotonic_ns']) / 1e6,
                              'sequence_step': b['sequence'] - a['sequence'],
                              'max_poll_ms': max((y - x) / 1e6 for x, y in zip(polls, polls[1:])
                                                 if x <= b['host_monotonic_ns'] and y >= a['host_monotonic_ns'])})

    def query(name, sql):
        r = subprocess.run([str(args.trace_processor.resolve()), 'query',
                            str(folder / 'guest.pftrace'), sql], capture_output=True, text=True)
        if r.returncode:
            raise RuntimeError(r.stderr)
        (folder / f'{name}.csv').write_text(r.stdout)
        return list(csv.DictReader(io.StringIO(r.stdout)))

    diagnostics = query('parser-stats', "SELECT name,value FROM stats WHERE value>0 AND "
                        "(name GLOB '*lost*' OR name GLOB '*error*' OR name GLOB '*overrun*' "
                        "OR name GLOB '*overwrit*' OR name GLOB '*discard*')")
    clock = query('clock-offset', "SELECT ts-clock_value AS offset_ns FROM clock_snapshot WHERE clock_name='MONOTONIC'")
    offsets = [int(r['offset_ns']) for r in clock]
    assert offsets and max(offsets) - min(offsets) < 1000000
    offset = round(statistics.median(offsets))
    # Perfetto normalizes to BOOTTIME; the fixture uses CLOCK_MONOTONIC.
    anchor_row = next(r for r in rows if r['frame'] == anchor)
    low = anchor_row['acquired_monotonic_ns'] + offset - 50000000
    high = low + 230000000
    latches = query('surface-latches', "SELECT ts,name FROM slice WHERE name GLOB 'latchBuffer*SurfaceView*dev.glidepod.vulkan*' "
                    f"AND ts BETWEEN {low} AND {high} ORDER BY ts")
    latch_deltas = [(int(b['ts']) - int(a['ts'])) / 1e6 for a, b in zip(latches, latches[1:])]
    slow = max(generation or [r for r in rows if r['sample'] >= 0], key=lambda r: r['acquire_ms'])
    begin = slow['loop_start_monotonic_ns'] + offset
    end = slow['acquired_monotonic_ns'] + offset
    slices = query('blocked-acquire', "SELECT s.ts,s.dur,s.name,s.id,s.parent_id,t.name AS thread FROM slice s "
                   "JOIN thread_track tt ON tt.id=s.track_id JOIN thread t ON t.utid=tt.utid "
                   f"WHERE t.name='GlidePod-Vulkan' AND s.ts BETWEEN {begin} AND {end} "
                   "AND (s.name IN ('GlidePod.vk.acquire','dequeueBuffer','waitForFreeSlotThenRelock') "
                   "OR s.name GLOB 'releaseBuffer*') ORDER BY s.ts")
    queue_wait = max((int(r['dur']) / 1e6 for r in slices if r['name'] == 'dequeueBuffer'), default=0)
    during = query('latches-during-acquire', "SELECT ts,name FROM slice WHERE name GLOB 'latchBuffer*SurfaceView*dev.glidepod.vulkan*' "
                   f"AND ts BETWEEN {begin} AND {end} ORDER BY ts")
    if recreating:
        query('swapchain-lifecycle', "SELECT s.ts,s.dur,s.name FROM slice s "
              "JOIN thread_track tt ON tt.id=s.track_id JOIN thread t ON t.utid=tt.utid "
              f"WHERE t.name='GlidePod-Vulkan' AND s.ts BETWEEN {low} AND {high} "
              "AND s.name IN ('GlidePod.vk.recreate','disconnect','connect') ORDER BY s.ts")
    result = {'renderer': report['renderer'], 'epoch': report['epoch'], 'display_id': report['display_id'],
              'recreate_requested': recreating, 'anchor_frame': anchor,
              'max_measured_producer_interval_ms': report['frame_interval']['max_ms'],
              'selected_acquire_ms': slow['acquire_ms'], 'nested_dequeue_ms': queue_wait,
              'explicit_recreation_ms': guest['explicit_swapchain_recreation_ms'],
              'host_window': near, 'host_window_complete': complete,
              'host_window_all_frames_consecutive': complete and all(r['frame_step'] == 1 for r in near),
              'host_window_max_ms': max(r['delta_ms'] for r in near),
              'host_window_min_ms': min(r['delta_ms'] for r in near),
              'surface_latch_count': len(latches), 'surface_latch_max_interval_ms': max(latch_deltas) if latch_deltas else None,
              'surface_latches_during_selected_acquire': len(during),
              'poll_max_ms': max(poll_deltas), 'host_window_poll_max_ms': max(near_poll_deltas) if near_poll_deltas else None,
              'all_measured_host_intervals': len(measured_gaps),
              'all_measured_host_max_ms': max(r['delta_ms'] for r in measured_gaps),
              'all_measured_host_gaps_over_33_33_ms': [r for r in measured_gaps if r['delta_ms'] > 1000 / 30],
              'clock_offset_ns': offset,
              'parser_diagnostics': diagnostics, 'frame_timeline_metrics_used': False,
              'ftrace_and_interning_integrity_ok': not any(r['name'] != 'frame_timeline_event_parser_errors' for r in diagnostics),
              'native_presentation_measured': False,
              'trace_sha256': hashlib.sha256((folder / 'guest.pftrace').read_bytes()).hexdigest()}
    (folder / 'analysis.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'host_window'}))


if __name__ == '__main__':
    main()
