// Validate CPU and PBO readback through the Metal backend, including untouched padding.
#include "../android/benchmark/native/stress.hpp"
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2ext.h>
#include <array>
#include <cstring>
#include <iostream>

using glidepod_stress::require;

int main() {
    try {
        auto get = reinterpret_cast<PFNEGLGETPLATFORMDISPLAYEXTPROC>(eglGetProcAddress("eglGetPlatformDisplayEXT"));
        require(get, "ANGLE platform unavailable");
        const EGLint attributes[] = {0x3203, 0x3489, EGL_NONE};
        auto display = get(0x3202, nullptr, attributes);
        EGLint major, minor, count;
        require(eglInitialize(display, &major, &minor), "Metal init failed");
        const EGLint configAttributes[] = {EGL_RENDERABLE_TYPE, 0x40, EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_NONE};
        EGLConfig config;
        require(eglChooseConfig(display, configAttributes, &config, 1, &count) && count, "No ES3 config");
        const EGLint contextAttributes[] = {EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE};
        auto context = eglCreateContext(display, config, EGL_NO_CONTEXT, contextAttributes);
        require(eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context), "Make current failed");
        const std::array<std::array<int, 4>, 4> colors = {{{1,0,0,1}, {0,1,0,1}, {0,0,1,1}, {1,1,1,1}}};
        GLuint fbo, pbo;
        glGenFramebuffers(1, &fbo); glBindFramebuffer(GL_FRAMEBUFFER, fbo);
        glGenBuffers(1, &pbo);
        glDisable(GL_DITHER);
        require(std::strstr(reinterpret_cast<const char*>(glGetString(GL_EXTENSIONS)),
                            "GL_ANGLE_pack_reverse_row_order"), "Reverse row extension unavailable");
        size_t checks = 0;
        for (const auto dimensions : {std::array<int,2>{65,49}, std::array<int,2>{513,773}})
        for (GLenum format : {GL_RGBA8, GL_RGB565, GL_RGBA4, GL_RGBA16F}) {
            GLuint texture;
            glGenTextures(1, &texture); glBindTexture(GL_TEXTURE_2D_ARRAY, texture);
            glTexStorage3D(GL_TEXTURE_2D_ARRAY, 2, format, dimensions[0], dimensions[1], 2);
            const bool floating = format == GL_RGBA16F;
            const size_t pixelBytes = floating ? 16 : 4;
            for (int level = 0; level < 2; ++level) for (int layer = 0; layer < 2; ++layer)
            for (int round = 0; round < 2; ++round) {
                const int width = dimensions[0] >> level, height = dimensions[1] >> level;
                glFramebufferTextureLayer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, texture, level, layer);
                require(glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE, "Incomplete readback FBO");
                glEnable(GL_SCISSOR_TEST);
                for (int quadrant = 0; quadrant < 4; ++quadrant) {
                    const auto& c = colors[(quadrant + layer + level + round) % 4];
                    const int x = quadrant % 2 ? width / 2 : 0, y = quadrant / 2 ? height / 2 : 0;
                    glScissor(x, y, quadrant % 2 ? width - x : width / 2, quadrant / 2 ? height - y : height / 2);
                    glClearColor(c[0], c[1], c[2], c[3]); glClear(GL_COLOR_BUFFER_BIT);
                }
                glDisable(GL_SCISSOR_TEST);
                for (bool subregion : {false, true}) for (bool packed : {false, true})
                for (bool usePbo : {false, true}) for (bool reversed : {false, true}) {
                    const int x = subregion ? 3 : 0, y = subregion ? 5 : 0;
                    const int w = width - (subregion ? 7 : 0), h = height - (subregion ? 11 : 0);
                    const int rowLength = packed ? w + 5 : w, skipPixels = packed ? 2 : 0, skipRows = packed ? 1 : 0;
                    const size_t alignment = packed ? 8 : 1;
                    const size_t pitch = (rowLength * pixelBytes + alignment - 1) / alignment * alignment;
                    const size_t size = pitch * (h + skipRows + 1) + 16;
                    std::vector<uint8_t> expected(size, 0xcd), actual(size, 0xcd);
                    for (int row = 0; row < h; ++row) for (int column = 0; column < w; ++column) {
                        int sourceRow = reversed ? h - row - 1 : row;
                        int quadrant = (x + column >= width / 2 ? 1 : 0) + (y + sourceRow >= height / 2 ? 2 : 0);
                        const auto& c = colors[(quadrant + layer + level + round) % 4];
                        auto* dst = expected.data() + (row + skipRows) * pitch + (column + skipPixels) * pixelBytes;
                        for (int channel = 0; channel < 4; ++channel) {
                            if (floating) { float value = static_cast<float>(c[channel]); std::memcpy(dst + channel * 4, &value, 4); }
                            else dst[channel] = c[channel] * 255;
                        }
                    }
                    glPixelStorei(GL_PACK_ALIGNMENT, alignment);
                    glPixelStorei(GL_PACK_ROW_LENGTH, packed ? rowLength : 0);
                    glPixelStorei(GL_PACK_SKIP_PIXELS, skipPixels); glPixelStorei(GL_PACK_SKIP_ROWS, skipRows);
                    glPixelStorei(GL_PACK_REVERSE_ROW_ORDER_ANGLE, reversed);
                    if (usePbo) {
                        glBindBuffer(GL_PIXEL_PACK_BUFFER, pbo);
                        glBufferData(GL_PIXEL_PACK_BUFFER, size, actual.data(), GL_STREAM_READ);
                    }
                    glReadPixels(x, y, w, h, GL_RGBA, floating ? GL_FLOAT : GL_UNSIGNED_BYTE,
                                 usePbo ? nullptr : actual.data());
                    if (usePbo) {
                        auto* data = glMapBufferRange(GL_PIXEL_PACK_BUFFER, 0, size, GL_MAP_READ_BIT);
                        require(data, "PBO map failed"); std::memcpy(actual.data(), data, size);
                        require(glUnmapBuffer(GL_PIXEL_PACK_BUFFER), "PBO contents invalid");
                        glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
                    }
                    const auto error = glGetError();
                    if (error || actual != expected) {
                        std::cerr << "Readback mismatch: format=" << format << " level=" << level << " layer=" << layer
                                  << " round=" << round << " subregion=" << subregion << " packed=" << packed
                                  << " pbo=" << usePbo << " reversed=" << reversed << " error=" << error << '\n';
                        return 1;
                    }
                    ++checks;
                }
            }
            glDeleteTextures(1, &texture);
        }
        glDeleteBuffers(1, &pbo); glDeleteFramebuffers(1, &fbo);
        eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
        eglDestroyContext(display, context); eglTerminate(display);
        std::cout << "{\"passed\":true,\"checks\":" << checks << "}\n";
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
