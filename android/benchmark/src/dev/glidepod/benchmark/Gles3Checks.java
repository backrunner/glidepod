package dev.glidepod.benchmark;

import android.opengl.GLES30;
import android.opengl.GLES31;
import java.nio.*;
import org.json.*;

/** Original pixel/readback checks, not a substitute for the Khronos conformance suite. */
final class Gles3Checks {
    static final int SIZE=64;
    static final String VERTEX="#version 300 es\nlayout(location=0) in vec2 p;layout(location=1) in vec4 color;flat out vec4 c;void main(){gl_Position=vec4(p,0,1);c=color;}";
    static final String FRAGMENT="#version 300 es\nprecision highp float;flat in vec4 c;out vec4 pixel;void main(){pixel=c;}";
    static final String FULL="#version 300 es\nvoid main(){vec2 p[3]=vec2[3](vec2(-1,-1),vec2(3,-1),vec2(-1,3));gl_Position=vec4(p[gl_VertexID],0,1);}";
    final JSONArray checks=new JSONArray();
    boolean passed=true;
    static boolean available(){
        android.opengl.EGLDisplay d=android.opengl.EGL14.eglGetDisplay(android.opengl.EGL14.EGL_DEFAULT_DISPLAY);
        int[] v=new int[2];if(!android.opengl.EGL14.eglInitialize(d,v,0,v,1))return false;
        try{android.opengl.EGLConfig[] c=new android.opengl.EGLConfig[1];int[] n=new int[1];return android.opengl.EGL14.eglChooseConfig(d,new int[]{android.opengl.EGL14.EGL_RENDERABLE_TYPE,0x40,android.opengl.EGL14.EGL_SURFACE_TYPE,android.opengl.EGL14.EGL_WINDOW_BIT,android.opengl.EGL14.EGL_NONE},0,c,0,1,n,0)&&n[0]>0;}
        finally{android.opengl.EGL14.eglTerminate(d);}
    }
    interface Check {void run() throws Exception;}
    void check(String name,Check action) throws Exception {
        JSONObject r=new JSONObject().put("name",name);
        while(GLES30.glGetError()!=GLES30.GL_NO_ERROR){}
        try{action.run();error();r.put("passed",true);}
        catch(Exception e){passed=false;r.put("passed",false).put("error",e.toString());ByteBuffer image=ByteBuffer.allocateDirect(SIZE*SIZE*4);GLES30.glReadPixels(0,0,SIZE,SIZE,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,image);byte[] data=new byte[image.capacity()];image.get(data);r.put("failure_rgba_base64",android.util.Base64.encodeToString(data,android.util.Base64.NO_WRAP));}
        checks.put(r);
    }
    static void error(){int e=GLES30.glGetError();if(e!=0)throw new IllegalStateException("GL error "+e);}
    static FloatBuffer floats(float... f){FloatBuffer b=ByteBuffer.allocateDirect(f.length*4).order(ByteOrder.nativeOrder()).asFloatBuffer();b.put(f).flip();return b;}
    static IntBuffer ints(int... i){IntBuffer b=ByteBuffer.allocateDirect(i.length*4).order(ByteOrder.nativeOrder()).asIntBuffer();b.put(i).flip();return b;}
    static int shader(int type,String source){int s=GLES30.glCreateShader(type);GLES30.glShaderSource(s,source);GLES30.glCompileShader(s);int[] ok=new int[1];GLES30.glGetShaderiv(s,GLES30.GL_COMPILE_STATUS,ok,0);if(ok[0]==0)throw new IllegalStateException(GLES30.glGetShaderInfoLog(s));return s;}
    static int program(String vs,String fs,String[] feedback){int v=shader(GLES30.GL_VERTEX_SHADER,vs),f=shader(GLES30.GL_FRAGMENT_SHADER,fs),p=GLES30.glCreateProgram();GLES30.glAttachShader(p,v);GLES30.glAttachShader(p,f);if(feedback!=null)GLES30.glTransformFeedbackVaryings(p,feedback,GLES30.GL_INTERLEAVED_ATTRIBS);GLES30.glLinkProgram(p);int[] ok=new int[1];GLES30.glGetProgramiv(p,GLES30.GL_LINK_STATUS,ok,0);if(ok[0]==0)throw new IllegalStateException(GLES30.glGetProgramInfoLog(p));GLES30.glDeleteShader(v);GLES30.glDeleteShader(f);return p;}
    static int buffer(int target,Buffer data,int bytes){int[] id=new int[1];GLES30.glGenBuffers(1,id,0);GLES30.glBindBuffer(target,id[0]);GLES30.glBufferData(target,bytes,data,GLES30.GL_STATIC_DRAW);return id[0];}
    static void vao(){int[] id=new int[1];GLES30.glGenVertexArrays(1,id,0);GLES30.glBindVertexArray(id[0]);}
    static void vertices(float... v){vao();buffer(GLES30.GL_ARRAY_BUFFER,floats(v),v.length*4);GLES30.glVertexAttribPointer(0,2,GLES30.GL_FLOAT,false,24,0);GLES30.glEnableVertexAttribArray(0);GLES30.glVertexAttribPointer(1,4,GLES30.GL_FLOAT,false,24,8);GLES30.glEnableVertexAttribArray(1);}
    static void pixel(int x,int y,int r,int g,int b){ByteBuffer p=ByteBuffer.allocateDirect(4);GLES30.glReadPixels(x,y,1,1,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,p);int[] actual={p.get(0)&255,p.get(1)&255,p.get(2)&255};int[] expected={r,g,b};for(int i=0;i<3;i++)if(Math.abs(actual[i]-expected[i])>2)throw new IllegalStateException("Pixel ("+x+","+y+") expected "+java.util.Arrays.toString(expected)+", got "+java.util.Arrays.toString(actual));}
    static void clear(){GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,0);GLES30.glViewport(0,0,SIZE,SIZE);GLES30.glClearColor(0,0,0,1);GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT);}
    static int texture(){int[] id=new int[1];GLES30.glGenTextures(1,id,0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,id[0]);GLES30.glTexStorage2D(GLES30.GL_TEXTURE_2D,1,GLES30.GL_RGBA8,SIZE,SIZE);return id[0];}
    static int framebuffer(){int[] id=new int[1];GLES30.glGenFramebuffers(1,id,0);GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,id[0]);return id[0];}
    static void complete(){if(GLES30.glCheckFramebufferStatus(GLES30.GL_FRAMEBUFFER)!=GLES30.GL_FRAMEBUFFER_COMPLETE)throw new IllegalStateException("Incomplete framebuffer");}
    static void fan(){vertices(0,0,1,0,0,1, -.8f,-.8f,0,1,0,1, .8f,-.8f,0,0,1,1, .8f,.8f,1,1,0,1, -.8f,.8f,1,0,1,1);}
    static void fanPixels(){pixel(32,16,0,0,255);pixel(48,32,255,255,0);pixel(32,48,255,0,255);}
    JSONObject run() throws Exception {
        GLES30.glDisable(GLES30.GL_DITHER);GLES30.glDisable(GLES30.GL_BLEND);GLES30.glDisable(GLES30.GL_DEPTH_TEST);GLES30.glDisable(GLES30.GL_CULL_FACE);
        int flat=program(VERTEX,FRAGMENT,null);GLES30.glUseProgram(flat);
        check("flat-triangle-last-vertex",()->{clear();vertices(-.8f,-.8f,1,0,0,1, .8f,-.8f,0,1,0,1, -.8f,.8f,0,0,1,1);GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,3);pixel(16,16,0,0,255);});
        check("flat-indexed-reversed-order",()->{clear();buffer(GLES30.GL_ELEMENT_ARRAY_BUFFER,ints(2,1,0),12);GLES30.glDrawElements(GLES30.GL_TRIANGLES,3,GLES30.GL_UNSIGNED_INT,0);pixel(16,16,255,0,0);});
        check("flat-triangle-strip",()->{clear();vertices(-.8f,-.8f,1,0,0,1, .8f,-.8f,0,1,0,1, -.8f,.8f,0,0,1,1, .8f,.8f,1,1,0,1);GLES30.glDrawArrays(GLES30.GL_TRIANGLE_STRIP,0,4);pixel(16,16,0,0,255);pixel(48,48,255,255,0);});
        check("flat-triangle-fan",()->{clear();vertices(0,0,1,0,0,1, -.8f,-.8f,0,1,0,1, .8f,-.8f,0,0,1,1, .8f,.8f,1,1,0,1, -.8f,.8f,1,0,1,1);GLES30.glDrawArrays(GLES30.GL_TRIANGLE_FAN,0,5);pixel(32,16,0,0,255);pixel(48,32,255,255,0);pixel(32,48,255,0,255);});
        check("flat-indexed-triangle-fan",()->{clear();fan();buffer(GLES30.GL_ELEMENT_ARRAY_BUFFER,ints(0,1,2,3,4),20);GLES30.glDrawElements(GLES30.GL_TRIANGLE_FAN,5,GLES30.GL_UNSIGNED_INT,0);fanPixels();});
        int[] minor=new int[1];GLES30.glGetIntegerv(GLES30.GL_MINOR_VERSION,minor,0);
        if(minor[0]>=1){
            check("flat-array-indirect-triangle-fan",()->{clear();fan();buffer(GLES31.GL_DRAW_INDIRECT_BUFFER,ints(5,1,0,0),16);GLES31.glDrawArraysIndirect(GLES30.GL_TRIANGLE_FAN,0);fanPixels();});
            check("flat-indexed-indirect-triangle-fan",()->{clear();fan();buffer(GLES30.GL_ELEMENT_ARRAY_BUFFER,ints(0,1,2,3,4),20);buffer(GLES31.GL_DRAW_INDIRECT_BUFFER,ints(5,1,0,0,0),20);GLES31.glDrawElementsIndirect(GLES30.GL_TRIANGLE_FAN,GLES30.GL_UNSIGNED_INT,0);fanPixels();});
        }
        check("flat-indexed-primitive-restart",()->{clear();vertices(-.8f,-.8f,1,0,0,1, .8f,-.8f,0,1,0,1, -.8f,.8f,0,0,1,1, .8f,.8f,1,1,0,1);buffer(GLES30.GL_ELEMENT_ARRAY_BUFFER,ints(0,1,2,-1,2,1,3),28);GLES30.glEnable(GLES30.GL_PRIMITIVE_RESTART_FIXED_INDEX);GLES30.glDrawElements(GLES30.GL_TRIANGLE_STRIP,7,GLES30.GL_UNSIGNED_INT,0);pixel(16,16,0,0,255);pixel(48,48,255,255,0);GLES30.glDisable(GLES30.GL_PRIMITIVE_RESTART_FIXED_INDEX);});
        check("integer-instancing-uniform-buffer",()->{
            clear();vao();int p=program("#version 300 es\nlayout(location=0) in vec2 p;layout(location=1) in uint code;layout(std140) uniform Colors {vec4 tint;};flat out vec4 c;void main(){gl_Position=vec4(p.x*.42+float(gl_InstanceID)*.9-.45,p.y*.8,0,1);c=vec4(float(code&1u),float((code>>1u)&1u),float((code>>2u)&1u),1)*tint;}",FRAGMENT,null);GLES30.glUseProgram(p);
            buffer(GLES30.GL_ARRAY_BUFFER,floats(-.8f,-.8f,.8f,-.8f,0,.8f),24);GLES30.glVertexAttribPointer(0,2,GLES30.GL_FLOAT,false,0,0);GLES30.glEnableVertexAttribArray(0);
            buffer(GLES30.GL_ARRAY_BUFFER,ints(1,2),8);GLES30.glVertexAttribIPointer(1,1,GLES30.GL_UNSIGNED_INT,0,0);GLES30.glEnableVertexAttribArray(1);GLES30.glVertexAttribDivisor(1,1);
            int u=buffer(GLES30.GL_UNIFORM_BUFFER,floats(.25f,.5f,.75f,1),16);GLES30.glUniformBlockBinding(p,GLES30.glGetUniformBlockIndex(p,"Colors"),0);GLES30.glBindBufferBase(GLES30.GL_UNIFORM_BUFFER,0,u);GLES30.glDrawArraysInstanced(GLES30.GL_TRIANGLES,0,3,2);pixel(17,25,64,0,0);pixel(46,25,0,128,0);
        });
        check("multiple-render-targets",()->{
            clear();vao();int f=framebuffer();for(int i=0;i<2;i++){int t=texture();GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0+i,GLES30.GL_TEXTURE_2D,t,0);}GLES30.glDrawBuffers(2,new int[]{GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_COLOR_ATTACHMENT1},0);complete();
            int p=program(FULL,"#version 300 es\nprecision highp float;layout(location=0) out vec4 a;layout(location=1) out vec4 b;void main(){a=vec4(.2,.4,.6,1);b=vec4(.8,.6,.4,1);}",null);GLES30.glUseProgram(p);GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,3);GLES30.glReadBuffer(GLES30.GL_COLOR_ATTACHMENT0);pixel(32,32,51,102,153);GLES30.glReadBuffer(GLES30.GL_COLOR_ATTACHMENT1);pixel(32,32,204,153,102);
        });
        check("texture-array-layer",()->{
            clear();vao();int[] id=new int[1];GLES30.glGenTextures(1,id,0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D_ARRAY,id[0]);GLES30.glTexStorage3D(GLES30.GL_TEXTURE_2D_ARRAY,1,GLES30.GL_RGBA8,1,1,2);ByteBuffer data=ByteBuffer.allocateDirect(8);data.put(new byte[]{(byte)255,0,0,(byte)255,0,(byte)255,0,(byte)255}).flip();GLES30.glTexSubImage3D(GLES30.GL_TEXTURE_2D_ARRAY,0,0,0,0,1,1,2,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,data);GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D_ARRAY,GLES30.GL_TEXTURE_MIN_FILTER,GLES30.GL_NEAREST);GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D_ARRAY,GLES30.GL_TEXTURE_MAG_FILTER,GLES30.GL_NEAREST);
            int p=program(FULL,"#version 300 es\nprecision highp float;uniform highp sampler2DArray image;out vec4 pixel;void main(){pixel=texture(image,vec3(.5,.5,1));}",null);GLES30.glUseProgram(p);GLES30.glUniform1i(GLES30.glGetUniformLocation(p,"image"),0);GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,3);pixel(32,32,0,255,0);
        });
        check("multisample-resolve",()->{
            clear();int src=framebuffer();int[] id=new int[1];GLES30.glGenRenderbuffers(1,id,0);GLES30.glBindRenderbuffer(GLES30.GL_RENDERBUFFER,id[0]);GLES30.glRenderbufferStorageMultisample(GLES30.GL_RENDERBUFFER,4,GLES30.GL_RGBA8,SIZE,SIZE);GLES30.glFramebufferRenderbuffer(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_RENDERBUFFER,id[0]);complete();GLES30.glClearColor(.2f,.4f,.6f,1);GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT);
            int dst=framebuffer();int t=texture();GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,t,0);complete();GLES30.glBindFramebuffer(GLES30.GL_READ_FRAMEBUFFER,src);GLES30.glBindFramebuffer(GLES30.GL_DRAW_FRAMEBUFFER,dst);GLES30.glBlitFramebuffer(0,0,SIZE,SIZE,0,0,SIZE,SIZE,GLES30.GL_COLOR_BUFFER_BIT,GLES30.GL_NEAREST);GLES30.glBindFramebuffer(GLES30.GL_READ_FRAMEBUFFER,dst);pixel(32,32,51,102,153);
        });
        check("transform-feedback-readback",()->{
            clear();vao();int p=program("#version 300 es\nout float captured;void main(){captured=float(gl_VertexID)*3.0+1.0;gl_Position=vec4(0,0,0,1);gl_PointSize=1.0;}","#version 300 es\nprecision highp float;out vec4 pixel;void main(){pixel=vec4(1);}",new String[]{"captured"});GLES30.glUseProgram(p);int b=buffer(GLES30.GL_TRANSFORM_FEEDBACK_BUFFER,null,12);GLES30.glBindBufferBase(GLES30.GL_TRANSFORM_FEEDBACK_BUFFER,0,b);GLES30.glEnable(GLES30.GL_RASTERIZER_DISCARD);GLES30.glBeginTransformFeedback(GLES30.GL_POINTS);GLES30.glDrawArrays(GLES30.GL_POINTS,0,3);GLES30.glEndTransformFeedback();GLES30.glDisable(GLES30.GL_RASTERIZER_DISCARD);
            Buffer mapped=GLES30.glMapBufferRange(GLES30.GL_TRANSFORM_FEEDBACK_BUFFER,0,12,GLES30.GL_MAP_READ_BIT);if(!(mapped instanceof ByteBuffer))throw new IllegalStateException("Missing mapped feedback");FloatBuffer data=((ByteBuffer)mapped).order(ByteOrder.nativeOrder()).asFloatBuffer();for(int i=0;i<3;i++)if(Math.abs(data.get(i)-(i*3+1))>.001)throw new IllegalStateException("Incorrect transform feedback");if(!GLES30.glUnmapBuffer(GLES30.GL_TRANSFORM_FEEDBACK_BUFFER))throw new IllegalStateException("Feedback buffer became invalid");
        });
        return new JSONObject().put("passed",passed).put("checks",checks);
    }
}
