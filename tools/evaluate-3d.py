#!/usr/bin/env python3
"""Measure a running disposable guest; never change the user's runtime preferences."""
import argparse, datetime, hashlib, json, pathlib, subprocess, time

ROOT = pathlib.Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--data-dir',type=pathlib.Path,required=True)
    parser.add_argument('--samples',type=int,default=180)
    parser.add_argument('--repeat',type=int,default=2)
    parser.add_argument('--gles',type=int,choices=[2,3],default=2)
    parser.add_argument('--api',choices=['gles','vulkan'],default='gles')
    parser.add_argument('--sustained-samples',type=int,default=0,help='Additional Vulkan heavy run, 600–3600 frames')
    parser.add_argument('--output',type=pathlib.Path,help='Evidence directory; defaults to a new timestamped run')
    args=parser.parse_args()
    data=args.data_dir.resolve()
    assert data.is_relative_to(ROOT/'.cache'), 'Use a disposable project-local store'
    assert 60<=args.samples<=600 and 1<=args.repeat<=5
    assert args.sustained_samples==0 or (args.api=='vulkan' and 600<=args.sustained_samples<=3600)
    cli=[str(ROOT/'target/debug/glidepodctl'),'--data-dir',str(data),'--json']
    status=json.loads(subprocess.check_output([*cli,'status'],text=True))['runtime']
    assert status['vm']['state']=='running'
    for package in ('dev.glidepod.fixture','dev.glidepod.fixture.secondary','dev.glidepod.widgets','dev.glidepod.benchmark','dev.glidepod.vulkan'):
        subprocess.run([*cli,'shell',f'am force-stop {package}'],check=True,stdout=subprocess.DEVNULL)
    renderer=status['effective_renderer']
    stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    label=f'gles{args.gles}' if args.api=='gles' else 'vulkan'
    out=(args.output or ROOT/'evidence/3d'/f'{stamp}-{renderer}-{label}').resolve()
    assert out.is_relative_to(ROOT/'evidence'), 'Keep evaluation output under project evidence'
    assert not out.exists(), 'Use a new evidence directory; existing measurements are immutable'
    out.mkdir(parents=True)
    cases=[('light-720',720,1080,1,True,False),('medium-720',720,1080,16,True,False),('heavy-720',720,1080,64,True,False),('medium-1080',1080,1920,16,True,False),('heavy-no-consumer',720,1080,64,False,False),('heavy-synchronized',720,1080,64,True,True)]
    if args.api=='vulkan':cases=cases[:-1]
    if args.sustained_samples:cases.append(('heavy-sustained',720,1080,64,True,False))
    results=[]
    def process_sample():
        fields=subprocess.check_output(['ps','-p',str(status['engine_pid']),'-o','cputime=,rss='],text=True).split()
        cpu=0.
        for part in fields[0].split(':'):cpu=cpu*60+float(part)
        return cpu,int(fields[1])/1024
    for name,width,height,objects,capture,finish in cases:
        for repeat in range(1 if name=='heavy-sustained' else args.repeat):
            key=f'{name}-{repeat+1}'
            samples=args.sustained_samples if name=='heavy-sustained' else args.samples
            cmd=[*cli,'benchmark','--api',args.api,'--width',str(width),'--height',str(height),'--objects',str(objects),'--samples',str(samples)]
            if args.api=='gles':cmd+=['--gles',str(args.gles)]
            if not capture:cmd+=['--no-capture']
            if finish:cmd+=['--finish-each-frame']
            cpu_start,_=process_sample();started=time.monotonic()
            result=subprocess.run(cmd,capture_output=True,text=True)
            wall=time.monotonic()-started;cpu_end,rss=process_sample()
            (out/f'{key}.log').write_text(result.stderr)
            if result.returncode:raise RuntimeError(f'{key}: {result.stderr}')
            report=json.loads(result.stdout)
            report['host_process']={'cpu_seconds':cpu_end-cpu_start,'wall_seconds':wall,'average_cpu_percent':100*(cpu_end-cpu_start)/wall,'rss_mib_end':rss,'scope':'entire VMM, including Android CPU and renderer; not GPU utilization'}
            assert report['epoch']==status['vm']['epoch'] and report['renderer']==renderer
            if args.api=='gles' and args.gles==3:assert report['guest']['gles3_probe']['correctness']['passed']
            if args.api=='vulkan':
                assert report['guest']['vk_success'] and not report['guest']['validation_enabled']
                assert report['display_id']>0 and report['guest']['swapchain_recreations']>=1
            (out/f'{key}.json').write_text(json.dumps(report,indent=2)+'\n')
            if capture:
                subprocess.run(['sips','-s','format','png',str(data/'developer/benchmark/latest.ppm'),'--out',str(out/f'{key}.png')],check=True,stdout=subprocess.DEVNULL)
            summary={'case':key,'frame_interval_scope':report.get('frame_interval_scope','unspecified'),'fps':report['mean_fps'],'frame_p95_ms':report['frame_interval']['p95_ms'],'draw_p95_ms':report['draw']['p95_ms'],'finish_each_frame':finish,'host_read_p95_ms':report['host_read']['p95_ms'] if report['host_read'] else None,'host_bgra_p95_ms':report['host_bgra_conversion']['p95_ms'] if report['host_bgra_conversion'] else None,'frames_over_33_33_ms':report['frames_over_33_33_ms']}
            summary['average_vmm_cpu_percent']=report['host_process']['average_cpu_percent']
            summary['device_timestamp_p95_ms']=report['device_timestamp']['p95_ms'] if report.get('device_timestamp') else None
            results.append(summary);print(json.dumps(summary),flush=True)
    host=json.loads(subprocess.check_output([str(ROOT/'target/debug/glidepodctl'),'--json','doctor'],text=True))
    apk='glidepod-vulkan.apk' if args.api=='vulkan' else 'glidepod-benchmark.apk'
    record={'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'host':host,'runtime':status,'benchmark_apk_sha256':hashlib.sha256((ROOT/'target/android'/apk).read_bytes()).hexdigest(),'samples_per_case':args.samples,'sustained_samples':args.sustained_samples,'warmup_seconds':2,'runs':results,'native_window_presentation_measured':False,'gpu_only_timing':False,'game_compatibility_qualified':False,'notes':['Draw timing measures CPU submission. GLES --finish-each-frame additionally waits with glFinish. Vulkan uses one submission in flight; frame intervals include fence completion and FIFO pacing.','Vulkan device timestamps are reported separately; their host translation is not calibrated as physical Metal GPU time.','No-consumer disables Rust frame copying only; upstream renderer readback/composition remains enabled.','The standalone suite excludes GPUI texture upload, compositor and physical display latency.','Vulkan intervals measure successful image acquisition, including swapchain recreation and queue backpressure. They are not display frame intervals.']}
    record['requested_api']=args.api
    if args.api=='gles':record['requested_gles']=args.gles
    if (data/'experimental-gpu.json').exists():record['registered_gpu']=json.loads((data/'experimental-gpu.json').read_text())
    (out/'report.json').write_text(json.dumps(record,indent=2)+'\n')

if __name__=='__main__':main()
