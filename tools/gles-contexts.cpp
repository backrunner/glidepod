// An idle context must not block an independent context's readback.
#include "../android/benchmark/native/stress.hpp"
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <chrono>
#include <future>
#include <iostream>
#include <thread>

using glidepod_stress::require;
struct Target {
    GLuint texture=0, fbo=0;
    Target() {
        glGenTextures(1,&texture); glBindTexture(GL_TEXTURE_2D,texture);
        glTexStorage2D(GL_TEXTURE_2D,1,GL_RGBA8,64,64);
        glGenFramebuffers(1,&fbo); glBindFramebuffer(GL_FRAMEBUFFER,fbo);
        glFramebufferTexture2D(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,texture,0);
        require(glCheckFramebufferStatus(GL_FRAMEBUFFER)==GL_FRAMEBUFFER_COMPLETE,"Incomplete target");
    }
    void clear(float red,float green) {
        glBindFramebuffer(GL_FRAMEBUFFER,fbo); glClearColor(red,green,0,1); glClear(GL_COLOR_BUFFER_BIT);
    }
    void check(int red,int green) {
        unsigned char p[4]={}; glReadPixels(32,32,1,1,GL_RGBA,GL_UNSIGNED_BYTE,p);
        auto error=glGetError();
        if(error || p[0]!=red || p[1]!=green || p[2]!=0 || p[3]!=255) {
            std::cerr<<"Expected "<<red<<','<<green<<"; actual "<<int(p[0])<<','<<int(p[1])<<','<<int(p[2])<<','<<int(p[3])<<" GL error "<<error<<std::endl;
            throw std::runtime_error("Incorrect context readback");
        }
    }
    ~Target() { glDeleteFramebuffers(1,&fbo); glDeleteTextures(1,&texture); }
};
int main(int argc,char**) {
    try {
        bool shared=argc>1;
        auto get=reinterpret_cast<PFNEGLGETPLATFORMDISPLAYEXTPROC>(eglGetProcAddress("eglGetPlatformDisplayEXT"));
        require(get,"ANGLE platform unavailable");
        const EGLint attr[]={0x3203,0x3489,EGL_NONE};
        auto display=get(0x3202,nullptr,attr); EGLint major,minor;
        require(eglInitialize(display,&major,&minor),"Metal init failed");
        EGLConfig config; EGLint count;
        const EGLint ca[]={EGL_RENDERABLE_TYPE,0x40,EGL_SURFACE_TYPE,EGL_PBUFFER_BIT,EGL_NONE};
        require(eglChooseConfig(display,ca,&config,1,&count)&&count,"No ES3 config");
        const EGLint ctx[]={EGL_CONTEXT_CLIENT_VERSION,3,EGL_NONE};
        auto a=eglCreateContext(display,config,EGL_NO_CONTEXT,ctx);
        auto b=eglCreateContext(display,config,shared?a:EGL_NO_CONTEXT,ctx);
        require(a!=EGL_NO_CONTEXT && b!=EGL_NO_CONTEXT,"Create context failed");
        require(eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,a),"Make A current failed");
        {
            Target first;
            for(int i=0;i<32;i++) {
                // Deliberately do not flush A. B uses a separate texture, so it has no dependency on A.
                first.clear(1,0);
                std::promise<void> result; auto completion=result.get_future();
                std::thread worker([&] {
                    try {
                        require(eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,b),"Make B current failed");
                        { Target second; second.clear(0,1); second.check(0,255); }
                        require(eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT),"Release B failed");
                        result.set_value();
                    } catch(...) { result.set_exception(std::current_exception()); }
                });
                if(completion.wait_for(std::chrono::seconds(5))!=std::future_status::ready) {
                    std::cerr<<"Independent context readback blocked by idle context\n";
                    std::_Exit(2); // Bound the regression; neither GL destruction nor join can unblock it.
                }
                worker.join(); completion.get();
                // Completion of B's newer serial must not mark A's pending work as complete.
                first.check(255,0);
                if(shared) {
                    // Explicitly synchronized texture ownership moves across the two contexts.
                    first.clear(1,0);
                    auto produced=glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE,0); glFlush();
                    GLsync consumed=nullptr;
                    std::promise<void> transferResult; auto transferred=transferResult.get_future();
                    std::thread consumer([&] {
                        try {
                            require(eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,b),"Make shared context current failed");
                            glWaitSync(produced,0,GL_TIMEOUT_IGNORED);
                            GLuint fbo; glGenFramebuffers(1,&fbo); glBindFramebuffer(GL_FRAMEBUFFER,fbo);
                            glFramebufferTexture2D(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,first.texture,0);
                            first.check(255,0);
                            glClearColor(0,1,0,1); glClear(GL_COLOR_BUFFER_BIT);
                            consumed=glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE,0); glFlush();
                            glDeleteFramebuffers(1,&fbo);
                            require(eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT),"Release shared context failed");
                            transferResult.set_value();
                        } catch(...) { transferResult.set_exception(std::current_exception()); }
                    });
                    if(transferred.wait_for(std::chrono::seconds(5))!=std::future_status::ready) {
                        std::cerr<<"Shared texture transfer timed out\n"; std::_Exit(2);
                    }
                    consumer.join(); transferred.get();
                    auto waited=glClientWaitSync(consumed,0,1000000000);
                    require(waited==GL_ALREADY_SIGNALED || waited==GL_CONDITION_SATISFIED,"Shared texture fence failed");
                    first.check(0,255); glDeleteSync(consumed); glDeleteSync(produced);
                }
            }
        }
        eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
        eglDestroyContext(display,b); eglDestroyContext(display,a); eglTerminate(display);
        std::cout<<"{\"passed\":true,\"iterations\":32,\"shared\":"<<(shared?"true":"false")<<"}\n";
        return 0;
    } catch(const std::exception& e) { std::cerr<<e.what()<<'\n'; return 1; }
}
