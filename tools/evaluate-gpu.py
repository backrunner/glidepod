#!/usr/bin/env python3
"""Evaluate an external gfxstream build in a disposable instance; never enable it in user settings."""
import datetime, json, os, pathlib, signal, socket, subprocess, tempfile, time
ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / 'evidence/g2/gpu'
OUT.mkdir(parents=True, exist_ok=True)
engine = ROOT / '.cache/upstream/avo/engine/hvf/target/release/avo-vmm'
renderer = ROOT / '.cache/upstream/avo/build/gfxstream'
instance = ROOT / '.cache/gpu-evaluation-instance'
with tempfile.TemporaryDirectory(prefix='gpgpu-', dir='/tmp') as sockets, (OUT / 'engine.log').open('w') as log:
    process = subprocess.Popen([str(engine),'cuttlefish',str(ROOT / '.cache/cuttlefish/16644040/images'),
        '--instance',str(instance),'--runtime',sockets,'--gpu','gfxstream','--gfxstream-dir',str(renderer),
        '--cpus','4','--mem','4096','--display','720x1080','--timeout','90','--exit-on-reboot','--no-microphone','--cameras','none'],
        stdout=log,stderr=subprocess.STDOUT,stdin=subprocess.DEVNULL,start_new_session=True,env=dict(os.environ,AVO_VMM_MUTE='1'))
    transcript = bytearray(); console = None; ready = False; diagnostic = False
    started = time.monotonic(); last = 0
    try:
        while process.poll() is None and time.monotonic()-started < 110:
            if console is None:
                try:
                    console = socket.socket(socket.AF_UNIX); console.settimeout(.2); console.connect(sockets+'/console.sock')
                except OSError:
                    if console: console.close()
                    console = None; time.sleep(.2); continue
            if time.monotonic()-last > 3:
                console.sendall(b'\necho GLIDEPOD_GPU_BOOT:$(getprop sys.boot_completed)\n'); last = time.monotonic()
            try: transcript.extend(console.recv(65536))
            except TimeoutError: pass
            if b'\nGLIDEPOD_GPU_BOOT:1' in transcript and not diagnostic:
                ready = True; diagnostic = True
                console.sendall(b'\ngetprop ro.hardware.egl; getprop ro.hardware.vulkan; dumpsys SurfaceFlinger; echo GLIDEPOD_GPU_DIAGNOSTICS_DONE\n')
            if diagnostic and b'\nGLIDEPOD_GPU_DIAGNOSTICS_DONE' in transcript:
                console.sendall(b'\nreboot -p\n'); break
        process.wait(timeout=20)
    finally:
        if process.poll() is None: os.killpg(process.pid,signal.SIGTERM); process.wait(timeout=10)
        if console: console.close()
        (OUT/'console.txt').write_bytes(transcript)
        result = {'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'boot_completed':ready,'exit_code':process.returncode,'duration_seconds':round(time.monotonic()-started,2),'external_renderer':str(renderer),'games_qualified':False,'production_enabled':False}
        (OUT/'report.json').write_text(json.dumps(result,indent=2)+'\n'); print(json.dumps(result,indent=2))
