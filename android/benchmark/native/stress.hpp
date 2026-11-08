#pragma once
// Original identical GLES workload for Android and the macOS ANGLE probe.
#include <GLES3/gl3.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <iomanip>
#include <numeric>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace glidepod_stress {
using Clock = std::chrono::steady_clock;
inline double ms(Clock::duration d) { return std::chrono::duration<double, std::milli>(d).count(); }
inline void require(bool ok, const std::string& message) { if (!ok) throw std::runtime_error(message); }
inline void check() { require(glGetError() == GL_NO_ERROR, "GLES error during stress workload"); }
inline GLuint shader(GLenum type, const std::string& text) {
    GLuint id = glCreateShader(type); const char* source = text.c_str();
    glShaderSource(id, 1, &source, nullptr); glCompileShader(id);
    GLint ok; glGetShaderiv(id, GL_COMPILE_STATUS, &ok);
    if (!ok) { char log[4096] = {}; glGetShaderInfoLog(id, sizeof(log), nullptr, log); glDeleteShader(id); throw std::runtime_error(log); }
    return id;
}
struct Resources {
    GLuint program = 0, target = 0, texture = 0, fbo = 0, vao = 0;
    ~Resources() {
        glBindFramebuffer(GL_FRAMEBUFFER, 0);
        glDeleteProgram(program); glDeleteTextures(1, &target); glDeleteTextures(1, &texture);
        glDeleteFramebuffers(1, &fbo); glDeleteVertexArrays(1, &vao);
    }
};
inline std::string stats(std::vector<double> values) {
    require(!values.empty(), "No stress samples");
    double mean = std::accumulate(values.begin(), values.end(), 0.0) / values.size();
    std::sort(values.begin(), values.end());
    auto p = [&](double f) { return values[static_cast<size_t>(std::ceil(f * values.size())) - 1]; };
    std::ostringstream out; out << std::setprecision(10) << "{\"mean\":" << mean << ",\"p50\":" << p(.5)
        << ",\"p95\":" << p(.95) << ",\"p99\":" << p(.99) << ",\"max\":" << values.back() << '}';
    return out.str();
}
inline std::string run(const std::string& mode, int width, int height, int seconds) {
    require(width >= 320 && width <= 2048 && height >= 320 && height <= 2048 && seconds >= 1 && seconds <= 120, "Invalid stress bounds");
    require(mode == "draw_calls" || mode == "fragment" || mode == "texture", "Unknown stress workload");
    const int draws = mode == "draw_calls" ? 4096 : 8;
    const int iterations = mode == "draw_calls" ? 1 : mode == "fragment" ? 128 : 32;
    Resources r;
    glDisable(GL_DEPTH_TEST); glDisable(GL_STENCIL_TEST); glDisable(GL_CULL_FACE);
    glDisable(GL_DITHER); glDisable(GL_SCISSOR_TEST); glDisable(GL_BLEND);
    glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
    glGenTextures(1, &r.target); glBindTexture(GL_TEXTURE_2D, r.target);
    glTexStorage2D(GL_TEXTURE_2D, 1, GL_RGBA8, width, height);
    glGenFramebuffers(1, &r.fbo); glBindFramebuffer(GL_FRAMEBUFFER, r.fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, r.target, 0);
    require(glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE, "Incomplete stress framebuffer");
    glViewport(0, 0, width, height);
    glClearColor(17.f/255, 83.f/255, 191.f/255, 1); glClear(GL_COLOR_BUFFER_BIT);
    unsigned char known[4] = {}; glReadPixels(0, 0, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, known);
    require(known[0] == 17 && known[1] == 83 && known[2] == 191 && known[3] == 255, "RGBA8 reference pixel mismatch");
    glGenTextures(1, &r.texture); glActiveTexture(GL_TEXTURE0); glBindTexture(GL_TEXTURE_2D, r.texture);
    std::vector<unsigned char> texture(1024*1024*4);
    for (int y=0; y<1024; ++y) for (int x=0; x<1024; ++x) {
        size_t at = static_cast<size_t>(y*1024+x)*4;
        texture[at] = (x*13+y*7)&255; texture[at+1] = (x*3+y*17)&255;
        texture[at+2] = (x*11+y*5)&255; texture[at+3] = 255;
    }
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, 1024, 1024, 0, GL_RGBA, GL_UNSIGNED_BYTE, texture.data());
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR); glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_REPEAT); glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_REPEAT);
    const std::string vs = "#version 300 es\nvoid main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));gl_Position=vec4(p*2.0-1.0,0,1);}";
    std::string fs = "#version 300 es\nprecision highp float; uniform vec2 size; uniform float seed; uniform float gain; uniform int iterations; uniform sampler2D image; out vec4 color; void main(){vec2 uv=gl_FragCoord.xy/size; vec3 v=vec3(uv,seed); for(int i=0;i<iterations;++i){";
    fs += mode == "texture" ? "v=texture(image,fract(uv+v.xy*0.021)).rgb;" : "v=fract(v.yzx*vec3(1.011,1.023,1.037)+vec3(0.013,0.017,0.019));";
    fs += "}color=vec4((vec3(0.2)+v*0.5)*gain,1.0);}";
    auto compile_start = Clock::now();
    auto v = shader(GL_VERTEX_SHADER, vs), f = shader(GL_FRAGMENT_SHADER, fs);
    r.program = glCreateProgram(); glAttachShader(r.program, v); glAttachShader(r.program, f); glLinkProgram(r.program);
    glDeleteShader(v); glDeleteShader(f); GLint linked; glGetProgramiv(r.program, GL_LINK_STATUS, &linked);
    require(linked, "Stress shader link failed");
    const double compile_ms = ms(Clock::now()-compile_start);
    glUseProgram(r.program); glGenVertexArrays(1, &r.vao); glBindVertexArray(r.vao);
    glUniform2f(glGetUniformLocation(r.program, "size"), width, height);
    glUniform1i(glGetUniformLocation(r.program, "iterations"), iterations);
    glUniform1i(glGetUniformLocation(r.program, "image"), 0);
    glUniform1f(glGetUniformLocation(r.program, "gain"), mode == "draw_calls" ? 1.f : 1.f/draws);
    const GLint seed = glGetUniformLocation(r.program, "seed");
    glEnable(GL_BLEND); glBlendEquation(GL_FUNC_ADD); glBlendFunc(GL_ONE, GL_ONE);
    auto frame = [&]() {
        glDisable(GL_SCISSOR_TEST); glClearColor(0,0,0,0); glClear(GL_COLOR_BUFFER_BIT);
        if (mode == "draw_calls") glEnable(GL_SCISSOR_TEST);
        for (int i=0;i<draws;++i) {
            if (mode == "draw_calls") {
                int x=i%64, y=i/64;
                glScissor(x*width/64, y*height/64, (x+1)*width/64-x*width/64, (y+1)*height/64-y*height/64);
            }
            glUniform1f(seed, float(i%31)/31);
            glDrawArrays(GL_TRIANGLES, 0, 3);
        }
    };
    check();
    const auto warm = Clock::now();
    do { frame(); glFinish(); check(); } while(ms(Clock::now()-warm)<2000);
    std::vector<double> completion, submission;
    completion.reserve(65536); submission.reserve(65536);
    const auto started = Clock::now();
    do {
        auto a=Clock::now(); frame(); auto b=Clock::now(); glFinish(); auto c=Clock::now(); check();
        completion.push_back(ms(c-a)); submission.push_back(ms(b-a));
        require(completion.size()<1000000, "Stress sample bound exceeded");
    } while(ms(Clock::now()-started)<seconds*1000.0);
    const double measured_ms = ms(Clock::now()-started);
    std::vector<unsigned char> pixels(static_cast<size_t>(width)*height*4);
    glReadPixels(0,0,width,height,GL_RGBA,GL_UNSIGNED_BYTE,pixels.data()); check();
    uint64_t hash=14695981039346656037ULL; size_t colored=0;
    for(auto byte:pixels){hash^=byte;hash*=1099511628211ULL;}
    for(size_t i=0;i<pixels.size();i+=4)if(pixels[i]+pixels[i+1]+pixels[i+2]>30)++colored;
    require(colored>static_cast<size_t>(width)*height/2, "Stress produced an empty image");
    auto section = [&](bool last) {
        size_t n=std::max<size_t>(1,completion.size()/4);
        auto begin=last?completion.end()-n:completion.begin();
        return std::accumulate(begin,begin+n,0.0)/n;
    };
    std::ostringstream out;
    out<<std::setprecision(10)<<"{\"workload\":"<<std::quoted(mode)<<",\"width\":"<<width<<",\"height\":"<<height
        <<",\"draws_per_frame\":"<<draws<<",\"shader_iterations\":"<<iterations<<",\"seconds_requested\":"<<seconds
        <<",\"warmup_seconds\":2,\"measured_ms\":"<<measured_ms<<",\"frames\":"<<completion.size()
        <<",\"completed_frames_per_second\":"<<completion.size()*1000.0/measured_ms
        <<",\"completion_ms\":"<<stats(completion)<<",\"submission_ms\":"<<stats(submission)
        <<",\"first_quarter_mean_ms\":"<<section(false)<<",\"last_quarter_mean_ms\":"<<section(true)
        <<",\"compile_link_ms\":"<<compile_ms<<",\"renderer\":"<<std::quoted(reinterpret_cast<const char*>(glGetString(GL_RENDERER)))
        <<",\"version\":"<<std::quoted(reinterpret_cast<const char*>(glGetString(GL_VERSION)))
        <<",\"colored_pixels\":"<<colored<<",\"reference_pixel_passed\":true,\"gl_error\":0,\"pixel_fnv1a64\":\""<<std::hex<<hash<<std::dec<<"\",\"pixel_probes\":[";
    bool comma=false;
    for(int y=1;y<=7;++y)for(int x=1;x<=7;++x)for(int c=0;c<4;++c){
        if(comma)out<<',';comma=true;out<<int(pixels[(static_cast<size_t>(y*height/8)*width+x*width/8)*4+c]);
    }
    out<<"],\"scope\":\"offscreen GLES command submission plus glFinish; no swap, scanout, readback or native presentation in timed loop\"}";
    return out.str();
}
}
