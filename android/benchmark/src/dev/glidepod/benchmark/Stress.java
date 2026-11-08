package dev.glidepod.benchmark;

import android.opengl.GLSurfaceView;
import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;
import org.json.JSONObject;

/** One bounded offscreen run; GLSurfaceView only owns the context, not the timed loop. */
final class Stress implements GLSurfaceView.Renderer {
    static { System.loadLibrary("glidepod_gles_stress"); }
    static native String nativeRun(String mode,int width,int height,int seconds);
    static native String nativeTopology();
    final GLSurfaceView view; final String run,mode; final int seconds;
    int width,height; boolean done; JSONObject correctness;
    Stress(GLSurfaceView v,String r,String m,int s){view=v;run=r;mode=m;seconds=s;}
    public void onSurfaceCreated(GL10 gl,EGLConfig cfg){
        try { correctness=new Gles3Checks().run(); }
        catch(Exception e){fail(e);}
    }
    public void onSurfaceChanged(GL10 gl,int w,int h){width=w;height=h;}
    public void onDrawFrame(GL10 gl){
        if(done)return;done=true;
        try {
            JSONObject result=new JSONObject(nativeRun(mode,width,height,seconds));
            result.put("ready",true).put("run",run).put("gles3_correctness",correctness);
            Results.json=result.toString();view.setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
        }catch(Exception e){fail(e);}
    }
    void fail(Exception e){done=true;try{Results.json=new JSONObject().put("ready",true).put("run",run).put("error",e.toString()).toString();}catch(Exception ignored){}}
}
