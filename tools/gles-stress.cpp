// Host ANGLE reference using the exact original Android workload.
#include "../android/benchmark/native/stress.hpp"
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <iostream>

int main(int argc, char** argv) {
    EGLDisplay display=EGL_NO_DISPLAY;
    try {
        glidepod_stress::require(argc==6,"usage: gles-stress metal|vulkan draw_calls|fragment|texture WIDTH HEIGHT SECONDS");
        std::string backend(argv[1]);
        glidepod_stress::require(backend=="metal"||backend=="vulkan","Unknown ANGLE backend");
        auto get=reinterpret_cast<PFNEGLGETPLATFORMDISPLAYEXTPROC>(eglGetProcAddress("eglGetPlatformDisplayEXT"));
        glidepod_stress::require(get,"ANGLE platform extension unavailable");
        EGLint attributes[]={0x3203,backend=="metal"?0x3489:0x3450,EGL_NONE};
        display=get(0x3202,nullptr,attributes);
        EGLint major,minor;
        glidepod_stress::require(eglInitialize(display,&major,&minor),"Requested ANGLE backend cannot initialize; no fallback was attempted");
        EGLConfig config; EGLint count;
        EGLint ca[]={EGL_RENDERABLE_TYPE,0x40,EGL_SURFACE_TYPE,EGL_PBUFFER_BIT,EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_ALPHA_SIZE,8,EGL_NONE};
        glidepod_stress::require(eglChooseConfig(display,ca,&config,1,&count)&&count>0,"ES 3 pbuffer unavailable");
        EGLint context_attributes[]={EGL_CONTEXT_CLIENT_VERSION,3,EGL_NONE};
        auto context=eglCreateContext(display,config,EGL_NO_CONTEXT,context_attributes);
        EGLint sa[]={EGL_WIDTH,16,EGL_HEIGHT,16,EGL_NONE};
        auto surface=eglCreatePbufferSurface(display,config,sa);
        glidepod_stress::require(eglMakeCurrent(display,surface,surface,context),"Could not make GLES context current");
        std::cout<<glidepod_stress::run(argv[2],std::stoi(argv[3]),std::stoi(argv[4]),std::stoi(argv[5]))<<std::endl;
        eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
        eglDestroySurface(display,surface);eglDestroyContext(display,context);eglTerminate(display);
        return 0;
    }catch(const std::exception& e){
        if(display!=EGL_NO_DISPLAY)eglTerminate(display);
        std::cerr<<e.what()<<" (EGL 0x"<<std::hex<<eglGetError()<<")\n";return 1;
    }
}
