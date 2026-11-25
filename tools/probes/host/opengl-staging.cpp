#include <glad/glad.h>
#include <GL/glx.h>
#include <X11/Xlib.h>
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <vector>
#include "common/logging.h"
#include "video_core/delayed_destruction_ring.h"
#include "video_core/renderer_opengl/gl_memory_budget.h"
#include "video_core/renderer_opengl/gl_staging_buffer_pool.h"

void AssertFailSoftImpl() { std::abort(); }
[[noreturn]] void AssertFatalImpl() { std::abort(); }

namespace Common::Log {
void FmtLogMessageImpl(Class, Level, const char*, unsigned int, const char*,
                      fmt::string_view format, const fmt::format_args& args) {
    std::printf("%s\n", fmt::vformat(format, args).c_str());
}
}

int main() {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    Display* display = XOpenDisplay(nullptr);
    if (!display) return 2;
    const int attributes[] = {GLX_RENDER_TYPE, GLX_RGBA_BIT, GLX_DRAWABLE_TYPE, GLX_PBUFFER_BIT, None};
    int count = 0;
    GLXFBConfig* configs = glXChooseFBConfig(display, DefaultScreen(display), attributes, &count);
    if (!configs || !count) return 3;
    const int context_attributes[] = {GLX_CONTEXT_MAJOR_VERSION_ARB, 4, GLX_CONTEXT_MINOR_VERSION_ARB, 5, None};
    auto create = reinterpret_cast<PFNGLXCREATECONTEXTATTRIBSARBPROC>(
        glXGetProcAddressARB(reinterpret_cast<const GLubyte*>("glXCreateContextAttribsARB")));
    if (!create) return 4;
    GLXContext context = create(display, configs[0], nullptr, True, context_attributes);
    const int pbuffer_attributes[] = {GLX_PBUFFER_WIDTH, 1, GLX_PBUFFER_HEIGHT, 1, None};
    GLXPbuffer pbuffer = glXCreatePbuffer(display, configs[0], pbuffer_attributes);
    if (!context || !glXMakeContextCurrent(display, pbuffer, pbuffer, context)) return 5;
    if (!gladLoadGLLoader(reinterpret_cast<GLADloadproc>(glXGetProcAddressARB))) return 6;
    std::printf("GL %s | %s\n", glGetString(GL_RENDERER), glGetString(GL_VERSION));
    setenv("CARAFE_GPU_CACHE_MIB", "512", 1);
    constexpr size_t mib = 1024 * 1024;
    unsigned failures = 0;
    const auto check = [&](const char* name, bool passed) {
        std::printf("%s %s\n", passed ? "PASS" : "FAIL", name);
        failures += !passed;
    };
    std::vector<unsigned char> expected(4096), actual(expected.size());
    for (size_t i = 0; i < expected.size(); ++i) expected[i] = static_cast<unsigned char>(i * 29 + 7);
    {
        OpenGL::OGLBuffer destination;
        destination.Create();
        glNamedBufferStorage(destination.handle, 64 * mib, nullptr, 0);
        OpenGL::StagingBuffers uploads{GL_MAP_WRITE_BIT, GL_MAP_WRITE_BIT | GL_MAP_FLUSH_EXPLICIT_BIT};
        {
            auto held = uploads.RequestMap(64 * mib, true);
            {
                auto alias = held;
                check("upload copies retain the CPU allocation", uploads.allocs[held.index].borrowers == 2);
            }
            check("intermediate copies do not fence later uploads", uploads.allocs[held.index].sync.handle == nullptr);
            std::copy(expected.begin(), expected.end(), held.mapped_span.begin());
            auto larger = uploads.RequestMap(128 * mib, true);
            check("pressure retains a live CPU mapping", uploads.allocs[held.index].size == 64 * mib);
            glFlushMappedNamedBufferRange(held.buffer, 0, expected.size());
            for (unsigned copy = 0; copy < 32; ++copy)
                glCopyNamedBufferSubData(held.buffer, destination.handle, 0, 0, 64 * mib);
        }
        {
            auto replacement = uploads.RequestMap(256 * mib, true);
            size_t reserved = 0;
            for (const auto& entry : uploads.allocs) reserved += entry.size;
            check("pressure reclaims completed idle allocations", reserved == 256 * mib);
            check("allocation slots are reused", uploads.allocs.size() == 2);
        }
        glFinish();
        glGetNamedBufferSubData(destination.handle, 0, actual.size(), actual.data());
        check("queued upload data survives reclamation", actual == expected);
        OpenGL::StagingBuffers downloads{GL_MAP_READ_BIT | GL_CLIENT_STORAGE_BIT, GL_MAP_READ_BIT};
        size_t deferred_index;
        {
            auto deferred = downloads.RequestMap(64 * mib, false, true);
            deferred_index = deferred.index;
            glCopyNamedBufferSubData(destination.handle, deferred.buffer, 0, 0, expected.size());
            glMemoryBarrier(GL_CLIENT_MAPPED_BUFFER_BARRIER_BIT);
            glFinish();
            check("download receives the queued data", std::equal(expected.begin(), expected.end(), deferred.mapped_span.begin()));
        }
        {
            auto larger = downloads.RequestMap(128 * mib, false, true);
            check("unconsumed downloads survive pressure without a CPU lease",
                  downloads.allocs[deferred_index].deferred && downloads.allocs[deferred_index].size == 64 * mib);
            downloads.FreeDeferredStagingBuffer(deferred_index);
            downloads.FreeDeferredStagingBuffer(larger.index);
            auto alias = larger;
            check("released downloads retain active CPU mappings", downloads.allocs[larger.index].borrowers == 2);
        }
        {
            auto replacement = downloads.RequestMap(256 * mib, false, true);
            size_t reserved = 0;
            for (const auto& entry : downloads.allocs) reserved += entry.size;
            check("consumed idle downloads are reclaimable", reserved == 256 * mib);
            downloads.FreeDeferredStagingBuffer(replacement.index);
        }
        check("no OpenGL API errors", glGetError() == GL_NO_ERROR);
    }
    {
        struct BufferAllocation {
            OpenGL::OGLBuffer buffer;
            size_t bytes;
            size_t SizeBytes() const { return bytes; }
        };
        constexpr size_t budget = 512 * mib;
        BufferAllocation active{{}, 64 * mib};
        active.buffer.Create();
        glNamedBufferData(active.buffer.handle, active.bytes, nullptr, GL_DYNAMIC_DRAW);
        glNamedBufferSubData(active.buffer.handle, 0, expected.size(), expected.data());
        VideoCommon::DelayedDestructionRing<BufferAllocation, 8> retired;
        unsigned reclamations = 0;
        size_t peak_bytes = active.bytes;
        for (unsigned iteration = 1; iteration <= 64; ++iteration) {
            const size_t incoming = 64 * mib + iteration * 65536;
            if (OpenGL::MemoryAccounting::ExceedsBudget(active.bytes + retired.SizeBytes(), incoming, budget)) {
                OpenGL::OGLSync completion;
                completion.Create();
                completion.Wait();
                retired.Clear();
                ++reclamations;
            }
            BufferAllocation replacement{{}, incoming};
            replacement.buffer.Create();
            glNamedBufferData(replacement.buffer.handle, incoming, nullptr, GL_DYNAMIC_DRAW);
            const unsigned zero = 0;
            glClearNamedBufferSubData(replacement.buffer.handle, GL_R32UI, 0, incoming,
                                     GL_RED_INTEGER, GL_UNSIGNED_INT, &zero);
            glCopyNamedBufferSubData(active.buffer.handle, replacement.buffer.handle, 0, 0, active.bytes);
            retired.Push(std::move(active));
            active = std::move(replacement);
            peak_bytes = std::max(peak_bytes, active.bytes + retired.SizeBytes());
        }
        check("buffer growth stays within its storage budget without presented frames", peak_bytes <= budget);
        check("buffer growth reclaims several generations without ticking the retirement ring", reclamations > 1);
        OpenGL::OGLSync completion;
        completion.Create();
        completion.Wait();
        retired.Clear();
        glGetNamedBufferSubData(active.buffer.handle, 0, actual.size(), actual.data());
        check("merged buffer data survives submission and retirement", actual == expected);
        check("buffer retirement produces no OpenGL API errors", glGetError() == GL_NO_ERROR);
        bool rejected_missing_fence = false;
        try {
            OpenGL::OGLSync missing;
            missing.Wait();
        } catch (const std::runtime_error&) {
            rejected_missing_fence = true;
        }
        check("a missing completion fence cannot authorize retirement", rejected_missing_fence);
        std::printf("BUFFER_RETIREMENT peak_bytes=%zu budget=%zu reclamations=%u\n",
                    peak_bytes, budget, reclamations);
    }
    std::printf("RESULT failures=%u\n", failures);
    glXMakeContextCurrent(display, None, None, nullptr);
    glXDestroyPbuffer(display, pbuffer);
    glXDestroyContext(display, context);
    XFree(configs);
    XCloseDisplay(display);
    return failures ? 1 : 0;
}
