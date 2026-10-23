package dev.glidepod.vulkan;

import android.app.Activity;
import android.os.Bundle;
import android.view.*;
import android.widget.TextView;
import org.json.JSONObject;

/** Direct Android libvulkan workload; no EGL, GLES or ANGLE calls. */
public final class MainActivity extends Activity implements SurfaceHolder.Callback {
    static { System.loadLibrary("glidepod_vulkan"); }
    private static native long createJob();
    private static native void cancelJob(long job);
    private static native void destroyJob(long job);
    private static native String execute(long job, Surface surface, int objects, int samples, boolean validation, boolean negative, boolean recreate, boolean stages);
    private final Object lock = new Object();
    private long job;
    private boolean started;
    private String run;
    private int objects, samples;
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        run = getIntent().getStringExtra("run");
        if (run == null) {
            TextView text = new TextView(this);
            text.setText("Run Check Vulkan from GlidePod Graphics.");
            setContentView(text); return;
        }
        objects = getIntent().getIntExtra("objects",16);
        samples = getIntent().getIntExtra("samples",180);
        if (!run.matches("[a-f0-9]{16,64}") || objects<1 || objects>64 || samples<60 || samples>3600)
            throw new IllegalArgumentException("Invalid Vulkan workload parameters");
        Results.json = "{\"ready\":false}";
        SurfaceView view = new SurfaceView(this);
        view.getHolder().addCallback(this);
        setContentView(view);
    }
    @Override public void surfaceCreated(SurfaceHolder holder) { }
    @Override public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        if (started) return;
        started = true;
        final long id;
        synchronized(lock) { id = createJob(); job = id; }
        new Thread(() -> {
            try {
                JSONObject result = new JSONObject(execute(id,holder.getSurface(),objects,samples,
                    getIntent().getBooleanExtra("validation",false),getIntent().getBooleanExtra("negative",false),
                    getIntent().getBooleanExtra("recreate",true),getIntent().getBooleanExtra("stages",false)));
                result.put("run",run).put("ready",true);
                Results.json = result.toString();
            } catch (Exception e) {
                try { Results.json = new JSONObject().put("ready",true).put("run",run).put("error",e.toString()).toString(); }
                catch (Exception ignored) { }
            } finally {
                synchronized(lock) { job = 0; destroyJob(id); }
            }
        },"GlidePod-Vulkan").start();
    }
    private void cancel() { synchronized(lock) { if(job!=0) cancelJob(job); } }
    @Override public void surfaceDestroyed(SurfaceHolder holder) { cancel(); }
    @Override protected void onDestroy() { cancel(); super.onDestroy(); }
}
