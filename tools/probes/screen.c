#include "probe.h"
#include <ddraw.h>

#define SCREEN_WIDTH 1024
#define SCREEN_HEIGHT 768

static LRESULT CALLBACK screen_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    static LPARAM last_move = -1;

    if (message == WM_MOUSEMOVE && lparam != last_move)
    {
        probe_log("move client %d,%d", (short)LOWORD(lparam), (short)HIWORD(lparam));
        last_move = lparam;
    }
    return probe_default_proc(window, message, wparam, lparam);
}

static void fill(LPDIRECTDRAWSURFACE primary)
{
    DDBLTFX effect;

    memset(&effect, 0, sizeof(effect));
    effect.dwSize = sizeof(effect);
    effect.dwFillColor = 0x7bef;
    IDirectDrawSurface_Blt(primary, NULL, NULL, NULL, DDBLT_COLORFILL | DDBLT_WAIT, &effect);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command, int show)
{
    WNDCLASSA klass = {0};
    LPDIRECTDRAW draw = NULL;
    LPDIRECTDRAWSURFACE primary = NULL;
    DDSURFACEDESC desc = {0};
    RECT client = {0};
    unsigned int frame = 0;
    HWND window;
    HRESULT hr;

    (void)previous;
    (void)command;
    (void)show;
    probe_open_log("screen");

    klass.lpfnWndProc = screen_proc;
    klass.hInstance = instance;
    klass.hCursor = LoadCursor(NULL, IDC_ARROW);
    klass.lpszClassName = "CarafeScreenProbe";
    RegisterClassA(&klass);
    window = CreateWindowExA(0, "CarafeScreenProbe", "Carafe screen probe", WS_POPUP | WS_VISIBLE, 0, 0,
                             SCREEN_WIDTH, SCREEN_HEIGHT, NULL, NULL, instance, NULL);
    if (!window)
    {
        probe_log("window failed: %lu", GetLastError());
        return 2;
    }

    hr = DirectDrawCreate(NULL, &draw, NULL);
    if (SUCCEEDED(hr))
        hr = IDirectDraw_SetCooperativeLevel(draw, window, DDSCL_EXCLUSIVE | DDSCL_FULLSCREEN);
    if (SUCCEEDED(hr))
        hr = IDirectDraw_SetDisplayMode(draw, SCREEN_WIDTH, SCREEN_HEIGHT, 16);
    if (SUCCEEDED(hr))
    {
        desc.dwSize = sizeof(desc);
        desc.dwFlags = DDSD_CAPS;
        desc.ddsCaps.dwCaps = DDSCAPS_PRIMARYSURFACE;
        hr = IDirectDraw_CreateSurface(draw, &desc, &primary, NULL);
    }
    if (FAILED(hr))
    {
        probe_log("DirectDraw failed: %#lx", hr);
        return 3;
    }
    probe_pump();
    GetClientRect(window, &client);
    probe_log("size %ldx%ld", client.right - client.left, client.bottom - client.top);

    while (probe_pump())
    {
        fill(primary);
        probe_frame_done(++frame);
        Sleep(16);
    }
    IDirectDrawSurface_Release(primary);
    IDirectDraw_Release(draw);
    probe_log("exit");
    return 0;
}
