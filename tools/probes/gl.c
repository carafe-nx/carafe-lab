#include "probe.h"

#include <GL/gl.h>

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    HDC dc;
    HGLRC context;
    PIXELFORMATDESCRIPTOR format = {0};
    int chosen;
    unsigned int frame = 0;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("gl");
    window = probe_window("Carafe OpenGL probe", probe_default_proc);
    dc = GetDC(window);
    format.nSize = sizeof(format);
    format.nVersion = 1;
    format.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
    format.iPixelType = PFD_TYPE_RGBA;
    format.cColorBits = 32;
    format.cDepthBits = 24;
    chosen = ChoosePixelFormat(dc, &format);
    if (!chosen || !SetPixelFormat(dc, chosen, &format))
    {
        probe_log("pixel format failed: %lu", GetLastError());
        return 3;
    }
    context = wglCreateContext(dc);
    if (!context || !wglMakeCurrent(dc, context))
    {
        probe_log("context failed: %lu", GetLastError());
        return 4;
    }
    probe_log("context ready: %s / %s", (const char *)glGetString(GL_RENDERER), (const char *)glGetString(GL_VERSION));
    glEnable(GL_SCISSOR_TEST);
    while (probe_pump())
    {
        int i;
        glViewport(0, 0, PROBE_WIDTH, PROBE_HEIGHT);
        for (i = 0; i < 4; i++)
        {
            const struct probe_quadrant *q = &probe_quadrants[i];
            glScissor(q->left, PROBE_HEIGHT - q->bottom, q->right - q->left, q->bottom - q->top);
            glClearColor(q->r, q->g, q->b, 1.0f);
            glClear(GL_COLOR_BUFFER_BIT);
        }
        SwapBuffers(dc);
        probe_frame_done(++frame);
    }
    wglMakeCurrent(NULL, NULL);
    wglDeleteContext(context);
    ReleaseDC(window, dc);
    probe_log("exit");
    return 0;
}
