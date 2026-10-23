package dev.glidepod.benchmark;

import android.app.Activity;
import android.os.Bundle;
import android.opengl.*;
import android.view.WindowManager;
import android.widget.TextView;
import java.nio.*;
import org.json.*;
import javax.microedition.khronos.egl.EGLConfig;
import javax.microedition.khronos.opengles.GL10;

/** Original deterministic GLES 2 scene: textured, lit, depth-tested 3D toruses. */
public final class MainActivity extends Activity {
    GLSurfaceView view;
    @Override public void onCreate(Bundle state){
        super.onCreate(state);getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        String run=getIntent().getStringExtra("run");
        if(run==null){TextView t=new TextView(this);t.setText("Run a 3D check from GlidePod Graphics.");setContentView(t);return;}
        int gles=getIntent().getIntExtra("gles",2);
        int objects=getIntent().getIntExtra("objects",16),samples=getIntent().getIntExtra("samples",180);
        if((gles!=2&&gles!=3)||!run.matches("[a-f0-9]{16,64}") || objects<1||objects>64||samples<60||samples>600)throw new IllegalArgumentException("Invalid benchmark parameters");
        Results.json="{\"ready\":false}";
        if(gles==3&&!Gles3Checks.available()){
            Results.json="{\"ready\":true,\"run\":\""+run+"\",\"error\":\"No ES 3 EGL configuration on this renderer\"}";
            TextView t=new TextView(this);t.setText("GLES 3 is unavailable on this renderer.");setContentView(t);return;
        }
        view=new GLSurfaceView(this);view.setEGLContextClientVersion(gles);view.setEGLConfigChooser(8,8,8,8,24,0);
        view.setRenderer(new Scene(run,gles,objects,samples,getIntent().getBooleanExtra("finish",false)));setContentView(view);
    }
    @Override protected void onPause(){if(view!=null)view.onPause();super.onPause();}
    @Override protected void onResume(){super.onResume();if(view!=null)view.onResume();}
    final class Scene implements GLSurfaceView.Renderer {
        final String run; final int gles,objects,samples; final boolean finish; int width,height,program,buffer,texture,frame;
        final JSONArray draw=new JSONArray(),interval=new JSONArray();
        long previous,first; boolean complete;
        String vendor,renderer,version,extensions;
        int p,n,uv,mvpLocation,mvLocation,sampler;
        JSONObject gles3;
        final float[] projection=new float[16],camera=new float[16],model=new float[16],mv=new float[16],mvp=new float[16];
        Scene(String r,int g,int o,int s,boolean f){run=r;gles=g;objects=o;samples=s;finish=f;}
        int shader(int type,String source){if(gles==3){source="#version 300 es\n"+source.replace("attribute ","in ").replace("varying ",type==GLES20.GL_VERTEX_SHADER?"out ":"in ").replace("texture2D(","texture(").replace("gl_FragColor","resultColor");if(type==GLES20.GL_FRAGMENT_SHADER)source=source.replace("void main()","out vec4 resultColor;void main()");}int s=GLES20.glCreateShader(type);GLES20.glShaderSource(s,source);GLES20.glCompileShader(s);int[] ok=new int[1];GLES20.glGetShaderiv(s,GLES20.GL_COMPILE_STATUS,ok,0);if(ok[0]==0)throw new IllegalStateException(GLES20.glGetShaderInfoLog(s));return s;}
        void fail(Exception e){complete=true;try{Results.json=new JSONObject().put("ready",true).put("run",run).put("error",e.toString()).toString();}catch(Exception ignored){}}
        JSONObject probeGles3() throws Exception {
            JSONObject result=new JSONObject().put("context_created",false);
            EGLDisplay d=EGL14.eglGetCurrentDisplay();EGLContext old=EGL14.eglGetCurrentContext();
            EGLSurface draw=EGL14.eglGetCurrentSurface(EGL14.EGL_DRAW),read=EGL14.eglGetCurrentSurface(EGL14.EGL_READ);
            EGLContext context=EGL14.EGL_NO_CONTEXT;EGLSurface surface=EGL14.EGL_NO_SURFACE;
            try {
                android.opengl.EGLConfig[] configs=new android.opengl.EGLConfig[1];int[] count=new int[1];
                if(!EGL14.eglChooseConfig(d,new int[]{EGL14.EGL_RENDERABLE_TYPE,0x40,EGL14.EGL_SURFACE_TYPE,EGL14.EGL_PBUFFER_BIT,EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_ALPHA_SIZE,8,EGL14.EGL_NONE},0,configs,0,1,count,0)||count[0]==0)return result.put("reason","No ES 3 EGL configuration");
                context=EGL14.eglCreateContext(d,configs[0],EGL14.EGL_NO_CONTEXT,new int[]{EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE},0);
                if(context.equals(EGL14.EGL_NO_CONTEXT))return result.put("egl_error",EGL14.eglGetError());
                surface=EGL14.eglCreatePbufferSurface(d,configs[0],new int[]{EGL14.EGL_WIDTH,Gles3Checks.SIZE,EGL14.EGL_HEIGHT,Gles3Checks.SIZE,EGL14.EGL_NONE},0);
                if(surface.equals(EGL14.EGL_NO_SURFACE)||!EGL14.eglMakeCurrent(d,surface,surface,context))return result.put("egl_error",EGL14.eglGetError());
                return result.put("context_created",true).put("version",GLES30.glGetString(GLES30.GL_VERSION)).put("correctness",new Gles3Checks().run());
            }finally{
                if(!EGL14.eglMakeCurrent(d,draw,read,old))throw new IllegalStateException("Could not restore benchmark context");
                if(!surface.equals(EGL14.EGL_NO_SURFACE))EGL14.eglDestroySurface(d,surface);
                if(!context.equals(EGL14.EGL_NO_CONTEXT))EGL14.eglDestroyContext(d,context);
            }
        }
        @Override public void onSurfaceCreated(GL10 unused,EGLConfig cfg){try{
            vendor=GLES20.glGetString(GLES20.GL_VENDOR);renderer=GLES20.glGetString(GLES20.GL_RENDERER);version=GLES20.glGetString(GLES20.GL_VERSION);extensions=GLES20.glGetString(GLES20.GL_EXTENSIONS);
            gles3=probeGles3();
            int vs=shader(GLES20.GL_VERTEX_SHADER,"attribute vec3 p;attribute vec3 n;attribute vec2 uv;uniform mat4 mvp;uniform mat4 mv;varying vec3 norm;varying vec2 tex;void main(){gl_Position=mvp*vec4(p,1.0);norm=mat3(mv)*n;tex=uv;}");
            int fs=shader(GLES20.GL_FRAGMENT_SHADER,"precision mediump float;varying vec3 norm;varying vec2 tex;uniform sampler2D image;void main(){vec3 n=normalize(norm);float l=max(dot(n,normalize(vec3(0.5,0.7,1.0))),0.0);float spec=pow(max(dot(n,normalize(vec3(0.1,0.3,1.0))),0.0),24.0);vec3 c=texture2D(image,tex).rgb;gl_FragColor=vec4(c*(0.18+0.82*l)+vec3(0.4)*spec,1.0);}");
            program=GLES20.glCreateProgram();GLES20.glAttachShader(program,vs);GLES20.glAttachShader(program,fs);GLES20.glLinkProgram(program);
            int[] ok=new int[1];GLES20.glGetProgramiv(program,GLES20.GL_LINK_STATUS,ok,0);if(ok[0]==0)throw new IllegalStateException(GLES20.glGetProgramInfoLog(program));
            GLES20.glDeleteShader(vs);GLES20.glDeleteShader(fs);
            p=GLES20.glGetAttribLocation(program,"p");n=GLES20.glGetAttribLocation(program,"n");uv=GLES20.glGetAttribLocation(program,"uv");
            mvpLocation=GLES20.glGetUniformLocation(program,"mvp");mvLocation=GLES20.glGetUniformLocation(program,"mv");sampler=GLES20.glGetUniformLocation(program,"image");
            FloatBuffer vertices=ByteBuffer.allocateDirect(32*16*6*8*4).order(ByteOrder.nativeOrder()).asFloatBuffer();
            int[] du={0,1,1,0,1,0},dv={0,0,1,0,1,1};
            for(int i=0;i<32;i++)for(int j=0;j<16;j++)for(int k=0;k<6;k++){
                double u=(i+du[k])*Math.PI*2/32,v=(j+dv[k])*Math.PI*2/16;
                float x=(float)Math.cos(u),y=(float)Math.sin(u),c=(float)Math.cos(v),s=(float)Math.sin(v);
                vertices.put(new float[]{x*(0.65f+0.25f*c),y*(0.65f+0.25f*c),0.25f*s,x*c,y*c,s,(i+du[k])/8f,(j+dv[k])/4f});
            }
            vertices.flip();int[] ids=new int[1];GLES20.glGenBuffers(1,ids,0);buffer=ids[0];GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER,buffer);GLES20.glBufferData(GLES20.GL_ARRAY_BUFFER,vertices.remaining()*4,vertices,GLES20.GL_STATIC_DRAW);
            GLES20.glGenTextures(1,ids,0);texture=ids[0];GLES20.glBindTexture(GLES20.GL_TEXTURE_2D,texture);
            ByteBuffer checker=ByteBuffer.allocateDirect(64*64*4);for(int y=0;y<64;y++)for(int x=0;x<64;x++){boolean a=((x/8+y/8)%2)==0;checker.put((byte)(a?90:215)).put((byte)(a?175:224)).put((byte)(a?125:184)).put((byte)255);}checker.flip();
            GLES20.glTexImage2D(GLES20.GL_TEXTURE_2D,0,GLES20.GL_RGBA,64,64,0,GLES20.GL_RGBA,GLES20.GL_UNSIGNED_BYTE,checker);
            GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D,GLES20.GL_TEXTURE_MIN_FILTER,GLES20.GL_LINEAR);GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D,GLES20.GL_TEXTURE_MAG_FILTER,GLES20.GL_LINEAR);
            GLES20.glEnable(GLES20.GL_DEPTH_TEST);GLES20.glEnable(GLES20.GL_CULL_FACE);GLES20.glClearColor(0.055f,0.075f,0.065f,1);
            Matrix.setLookAtM(camera,0,0,0,5,0,0,0,0,1,0);
        }catch(Exception e){fail(e);}}
        @Override public void onSurfaceChanged(GL10 unused,int w,int h){width=w;height=h;GLES20.glViewport(0,0,w,h);Matrix.perspectiveM(projection,0,45,(float)w/h,0.1f,50);}
        @Override public void onDrawFrame(GL10 unused){if(complete)return;try{
            long start=System.nanoTime();if(first==0)first=start;
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT|GLES20.GL_DEPTH_BUFFER_BIT);GLES20.glUseProgram(program);GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER,buffer);
            GLES20.glEnableVertexAttribArray(p);GLES20.glEnableVertexAttribArray(n);GLES20.glEnableVertexAttribArray(uv);
            GLES20.glVertexAttribPointer(p,3,GLES20.GL_FLOAT,false,32,0);GLES20.glVertexAttribPointer(n,3,GLES20.GL_FLOAT,false,32,12);GLES20.glVertexAttribPointer(uv,2,GLES20.GL_FLOAT,false,32,24);
            GLES20.glBindTexture(GLES20.GL_TEXTURE_2D,texture);GLES20.glUniform1i(sampler,0);
            int grid=(int)Math.ceil(Math.sqrt(objects));float step=2.5f/grid,scale=1.15f/grid;
            for(int i=0;i<objects;i++){
                Matrix.setIdentityM(model,0);Matrix.translateM(model,0,((i%grid)-(grid-1)*0.5f)*step,((i/grid)-(grid-1)*0.5f)*step,0);
                Matrix.rotateM(model,0,frame*0.8f+i*13,1,0.7f,0.2f);Matrix.scaleM(model,0,scale,scale,scale);
                Matrix.multiplyMM(mv,0,camera,0,model,0);Matrix.multiplyMM(mvp,0,projection,0,mv,0);
                GLES20.glUniformMatrix4fv(mvpLocation,1,false,mvp,0);GLES20.glUniformMatrix4fv(mvLocation,1,false,mv,0);
                GLES20.glDrawArrays(GLES20.GL_TRIANGLES,0,32*16*6);
            }
            if(finish)GLES20.glFinish();long end=System.nanoTime();int error=GLES20.glGetError();if(error!=GLES20.GL_NO_ERROR)throw new IllegalStateException("GL error "+error);
            if(start-first>=2_000_000_000L && previous!=0){draw.put((end-start)/1e6);interval.put((start-previous)/1e6);}
            previous=start;frame++;
            if(draw.length()==samples){
                ByteBuffer pixel=ByteBuffer.allocateDirect(width*height*4);GLES20.glReadPixels(0,0,width,height,GLES20.GL_RGBA,GLES20.GL_UNSIGNED_BYTE,pixel);int changed=0;
                for(int i=0;i<width*height;i++){int green=pixel.get(i*4+1)&255;if(green>40)changed++;}
                JSONObject o=new JSONObject().put("ready",true).put("run",run).put("vendor",vendor).put("renderer",renderer).put("version",version).put("extensions",extensions)
                    .put("requested_gles",gles).put("width",width).put("height",height).put("objects",objects).put("triangles_per_frame",objects*1024).put("warmup_seconds",2).put("gles3_probe",gles3)
                    .put("finish_each_frame",finish).put("draw_ms",draw).put("frame_interval_ms",interval).put("colored_pixels",changed).put("gl_error",GLES20.glGetError());
                Results.json=o.toString();complete=true;view.setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
            }
        }catch(Exception e){fail(e);}}
    }
}
