#ifndef CARAFE_PROBE_H
#define CARAFE_PROBE_H

#include <windows.h>
#include <stdarg.h>
#include <stdio.h>

#define PROBE_WIDTH 1280
#define PROBE_HEIGHT 720

static FILE *probe_log_file;
static LARGE_INTEGER probe_start;
static LARGE_INTEGER probe_frequency;

static void probe_log(const char *format, ...)
{
    LARGE_INTEGER now;
    va_list args;
    char line[512];
    int prefix;

    QueryPerformanceCounter(&now);
    prefix = snprintf(line, sizeof(line), "%.3f ",
                      (double)(now.QuadPart - probe_start.QuadPart) / (double)probe_frequency.QuadPart);
    va_start(args, format);
    vsnprintf(line + prefix, sizeof(line) - prefix, format, args);
    va_end(args);
    OutputDebugStringA(line);
    OutputDebugStringA("\n");
    if (probe_log_file)
    {
        fprintf(probe_log_file, "%s\n", line);
        fflush(probe_log_file);
    }
}

static void probe_open_log(const char *name)
{
    char path[MAX_PATH];
    char *slash;
    DWORD length;

    QueryPerformanceFrequency(&probe_frequency);
    QueryPerformanceCounter(&probe_start);
    length = GetModuleFileNameA(NULL, path, sizeof(path));
    slash = length ? strrchr(path, '\\') : NULL;
    if (slash && (size_t)(slash - path) + 32 < sizeof(path))
    {
        snprintf(slash + 1, sizeof(path) - (size_t)(slash + 1 - path), "probe-%s.log", name);
        probe_log_file = fopen(path, "w");
    }
    if (!probe_log_file)
    {
        char temp[MAX_PATH];
        if (GetTempPathA(sizeof(temp), temp))
        {
            snprintf(path, sizeof(path), "%sprobe-%s.log", temp, name);
            probe_log_file = fopen(path, "w");
        }
    }
    probe_log("probe %s ready, %d-bit, log %s", name, (int)(sizeof(void *) * 8), probe_log_file ? path : "none");
}

static HWND probe_window(const char *title, WNDPROC proc)
{
    WNDCLASSA klass = {0};
    RECT rect = {0, 0, PROBE_WIDTH, PROBE_HEIGHT};
    HWND window;

    klass.lpfnWndProc = proc;
    klass.hInstance = GetModuleHandleA(NULL);
    klass.hCursor = LoadCursor(NULL, IDC_ARROW);
    klass.lpszClassName = "CarafeProbe";
    klass.style = CS_OWNDC;
    RegisterClassA(&klass);
    AdjustWindowRect(&rect, WS_OVERLAPPEDWINDOW, FALSE);
    window = CreateWindowA("CarafeProbe", title, WS_OVERLAPPEDWINDOW | WS_VISIBLE, 0, 0,
                           rect.right - rect.left, rect.bottom - rect.top, NULL, NULL, klass.hInstance, NULL);
    if (!window)
    {
        probe_log("window failed: %lu", GetLastError());
        ExitProcess(2);
    }
    ShowWindow(window, SW_SHOW);
    SetForegroundWindow(window);
    SetFocus(window);
    probe_log("window %p", (void *)window);
    return window;
}

static BOOL probe_pump(void)
{
    MSG message;

    while (PeekMessageA(&message, NULL, 0, 0, PM_REMOVE))
    {
        if (message.message == WM_QUIT)
            return FALSE;
        TranslateMessage(&message);
        DispatchMessageA(&message);
    }
    return TRUE;
}

static LRESULT CALLBACK probe_default_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_CLOSE)
    {
        probe_log("close requested");
        PostQuitMessage(0);
        return 0;
    }
    if (message == WM_DESTROY)
    {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcA(window, message, wparam, lparam);
}

struct probe_quadrant
{
    LONG left, top, right, bottom;
    float r, g, b;
};

static const struct probe_quadrant probe_quadrants[4] =
{
    {0, 0, PROBE_WIDTH / 2, PROBE_HEIGHT / 2, 0.85f, 0.10f, 0.10f},
    {PROBE_WIDTH / 2, 0, PROBE_WIDTH, PROBE_HEIGHT / 2, 0.10f, 0.75f, 0.20f},
    {0, PROBE_HEIGHT / 2, PROBE_WIDTH / 2, PROBE_HEIGHT, 0.15f, 0.25f, 0.90f},
    {PROBE_WIDTH / 2, PROBE_HEIGHT / 2, PROBE_WIDTH, PROBE_HEIGHT, 0.95f, 0.95f, 0.95f},
};

static inline void probe_frame_done(unsigned int frame)
{
    if (frame == 1 || frame % 300 == 0)
        probe_log("presented %u frames", frame);
}

#endif
