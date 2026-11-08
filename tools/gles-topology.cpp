// Isolate GLES topology translation from gfxstream and the Android guest.
#include "../android/benchmark/native/topology.hpp"
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <array>
#include <iostream>

int main() {
    EGLDisplay display=EGL_NO_DISPLAY;
    try {
        auto get=reinterpret_cast<PFNEGLGETPLATFORMDISPLAYEXTPROC>(eglGetProcAddress("eglGetPlatformDisplayEXT"));
        glidepod_stress::require(get,"ANGLE platform unavailable");
        const EGLint attr[]={0x3203,0x3489,EGL_NONE};
        display=get(0x3202,nullptr,attr);
        EGLint major,minor; glidepod_stress::require(eglInitialize(display,&major,&minor),"Metal init failed");
        EGLConfig config; EGLint count;
        const EGLint ca[]={EGL_RENDERABLE_TYPE,0x40,EGL_SURFACE_TYPE,EGL_PBUFFER_BIT,EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_ALPHA_SIZE,8,EGL_NONE};
        glidepod_stress::require(eglChooseConfig(display,ca,&config,1,&count)&&count,"No ES3 config");
        const EGLint ctx[]={EGL_CONTEXT_CLIENT_VERSION,3,EGL_NONE}, sa[]={EGL_WIDTH,16,EGL_HEIGHT,16,EGL_NONE};
        auto context=eglCreateContext(display,config,EGL_NO_CONTEXT,ctx);
        auto surface=eglCreatePbufferSurface(display,config,sa);
        glidepod_stress::require(eglMakeCurrent(display,surface,surface,context),"Make current failed");
        const auto result=glidepod_topology::run();
        std::cout<<result.json<<std::endl;
        eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
        eglDestroySurface(display,surface); eglDestroyContext(display,context); eglTerminate(display);
        return result.passed?0:1;
    } catch(const std::exception& e) {
        if(display!=EGL_NO_DISPLAY) eglTerminate(display);
        std::cerr<<e.what()<<'\n'; return 1;
    }
}
