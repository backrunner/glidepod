#pragma once
// Shared host/guest regression matrix for index translation and flat shading.
#include "stress.hpp"
#include <array>

namespace glidepod_topology {
struct Result { bool passed; std::string json; };
inline Result run() {
    using namespace glidepod_stress;
    Resources r;
    glGenTextures(1, &r.target); glBindTexture(GL_TEXTURE_2D, r.target);
    glTexStorage2D(GL_TEXTURE_2D, 1, GL_RGBA8, 64, 64);
    glGenFramebuffers(1, &r.fbo); glBindFramebuffer(GL_FRAMEBUFFER, r.fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, r.target, 0);
    require(glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE, "Incomplete topology FBO");
    auto v = shader(GL_VERTEX_SHADER, "#version 300 es\nlayout(location=0) in vec2 p;layout(location=1) in vec4 color;flat out vec4 c;void main(){gl_Position=vec4(p,0,1);c=color;}");
    auto f = shader(GL_FRAGMENT_SHADER, "#version 300 es\nprecision highp float;flat in vec4 c;out vec4 pixel;void main(){pixel=c;}");
    r.program = glCreateProgram(); glAttachShader(r.program, v); glAttachShader(r.program, f);
    glLinkProgram(r.program); glDeleteShader(v); glDeleteShader(f);
    GLint linked; glGetProgramiv(r.program, GL_LINK_STATUS, &linked); require(linked, "Topology link failed");
    glUseProgram(r.program); glViewport(0, 0, 64, 64);
    glDisable(GL_DITHER); glDisable(GL_BLEND); glDisable(GL_DEPTH_TEST);
    glDisable(GL_SCISSOR_TEST); glDisable(GL_RASTERIZER_DISCARD);
    glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
    glGenVertexArrays(1, &r.vao); glBindVertexArray(r.vao);
    GLuint buffers[2]; glGenBuffers(2, buffers);
    glBindBuffer(GL_ARRAY_BUFFER, buffers[0]);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 24, nullptr); glEnableVertexAttribArray(0);
    glVertexAttribPointer(1, 4, GL_FLOAT, GL_FALSE, 24, reinterpret_cast<void*>(8)); glEnableVertexAttribArray(1);
    glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, buffers[1]);
    const std::array<uint8_t,1024> empty{};
    glBufferData(GL_ELEMENT_ARRAY_BUFFER, empty.size(), empty.data(), GL_DYNAMIC_DRAW);
    const float fan[] = {0,0,1,0,0,1, -.8f,-.8f,0,1,0,1, .8f,-.8f,0,0,1,1, .8f,.8f,1,1,0,1, -.8f,.8f,1,0,1,1};
    const float strip[] = {-.8f,-.8f,1,0,0,1, .8f,-.8f,0,1,0,1, -.8f,.8f,0,0,1,1, .8f,.8f,1,1,0,1};
    constexpr uint32_t restart = ~0u;
    struct Case { const char* name; bool fan, restart; std::vector<uint32_t> ids; };
    const std::vector<Case> cases = {
        {"fan",true,false,{0,1,2,3,4}},
        {"fan-restart",true,true,{0,1,2,restart,0,2,3,4}},
        {"fan-restart-boundaries",true,true,{restart,restart,0,1,2,restart,restart,0,2,3,4,restart}},
        {"strip",false,false,{0,1,2,3}},
        {"strip-restart",false,true,{0,1,2,restart,2,1,3}},
        {"strip-restart-boundaries",false,true,{restart,0,1,2,restart,restart,2,1,3,restart}},
    };
    bool allPassed = true; size_t total=0, failures=0;
    std::ostringstream out;
    out<<"{\"renderer\":"<<std::quoted(reinterpret_cast<const char*>(glGetString(GL_RENDERER)))<<",\"checks\":[";
    for (const auto& c : cases) for (GLenum type : {GL_UNSIGNED_BYTE, GL_UNSIGNED_SHORT, GL_UNSIGNED_INT})
    for (int method=0;method<3;method++) for (int prefix : {0,3}) for (bool cull : {false,true}) {
        glBufferData(GL_ARRAY_BUFFER, c.fan ? sizeof(fan) : sizeof(strip), c.fan ? fan : strip, GL_STATIC_DRAW);
        const size_t size = type==GL_UNSIGNED_BYTE ? 1 : type==GL_UNSIGNED_SHORT ? 2 : 4;
        // A persistent buffer plus subdata updates exercises converted-index cache invalidation.
        // Leading/trailing sentinels outside the draw window must not affect its topology.
        std::vector<uint32_t> source(prefix, restart);
        source.insert(source.end(),c.ids.begin(),c.ids.end()); source.push_back(restart);
        std::vector<uint8_t> bytes(source.size()*size);
        for(size_t i=0;i<source.size();i++) for(size_t j=0;j<size;j++) bytes[i*size+j]=(source[i]>>(8*j))&255;
        glBufferSubData(GL_ELEMENT_ARRAY_BUFFER,0,bytes.size(),bytes.data());
        if(c.restart) glEnable(GL_PRIMITIVE_RESTART_FIXED_INDEX); else glDisable(GL_PRIMITIVE_RESTART_FIXED_INDEX);
        if(cull) glEnable(GL_CULL_FACE); else glDisable(GL_CULL_FACE);
        glCullFace(GL_BACK); glFrontFace(GL_CCW);
        glClearColor(0,0,0,1); glClear(GL_COLOR_BUFFER_BIT);
        auto offset=reinterpret_cast<const void*>(prefix*size);
        GLenum mode=c.fan?GL_TRIANGLE_FAN:GL_TRIANGLE_STRIP;
        if(method==0) glDrawElements(mode,c.ids.size(),type,offset);
        else if(method==1) glDrawElementsInstanced(mode,c.ids.size(),type,offset,2);
        else glDrawRangeElements(mode,0,c.fan?4:3,c.ids.size(),type,offset);
        const std::vector<std::array<int,5>> probes=c.fan
            ? std::vector<std::array<int,5>>{{32,16,0,0,255},{48,32,255,255,0},{32,48,255,0,255}}
            : std::vector<std::array<int,5>>{{16,16,0,0,255},{48,48,255,255,0}};
        bool correct=true; std::ostringstream actual;
        for(auto p:probes) {
            uint8_t pixel[4]={}; glReadPixels(p[0],p[1],1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
            for(int channel=0;channel<3;channel++){correct &= std::abs(int(pixel[channel])-p[channel+2])<=2; actual<<int(pixel[channel])<<' ';}
        }
        auto error=glGetError(); correct &= error==GL_NO_ERROR; allPassed &= correct; if(!correct)++failures;
        if(total++)out<<',';
        out<<"{\"case\":"<<std::quoted(c.name)<<",\"index_bytes\":"<<size<<",\"method\":"<<method
            <<",\"prefix_indices\":"<<prefix<<",\"cull\":"<<(cull?"true":"false")<<",\"passed\":"<<(correct?"true":"false");
        if(!correct)out<<",\"gl_error\":"<<error<<",\"actual_rgb\":"<<std::quoted(actual.str());
        out<<'}';
    }
    glDisable(GL_PRIMITIVE_RESTART_FIXED_INDEX); glDisable(GL_CULL_FACE);
    glBindVertexArray(0); glDeleteBuffers(2,buffers); glUseProgram(0);
    out<<"],\"total\":"<<total<<",\"failures\":"<<failures<<",\"passed\":"<<(allPassed?"true":"false")<<'}';
    return {allPassed,out.str()};
}
}
