#define COBJMACROS
#include "probe.h"

#include <d3d9.h>

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    IDirect3D9 *d3d;
    IDirect3DDevice9 *device = NULL;
    D3DPRESENT_PARAMETERS params = {0};
    HRESULT hr;
    unsigned int frame = 0;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("d3d9");
    window = probe_window("Carafe D3D9 probe", probe_default_proc);
    d3d = Direct3DCreate9(D3D_SDK_VERSION);
    if (!d3d)
    {
        probe_log("Direct3DCreate9 failed");
        return 3;
    }
    params.Windowed = TRUE;
    params.SwapEffect = D3DSWAPEFFECT_DISCARD;
    params.BackBufferFormat = D3DFMT_X8R8G8B8;
    params.BackBufferWidth = PROBE_WIDTH;
    params.BackBufferHeight = PROBE_HEIGHT;
    params.hDeviceWindow = window;
    params.PresentationInterval = D3DPRESENT_INTERVAL_ONE;
    hr = IDirect3D9_CreateDevice(d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
                                 D3DCREATE_HARDWARE_VERTEXPROCESSING, &params, &device);
    if (FAILED(hr))
    {
        probe_log("CreateDevice failed: 0x%08lx", (unsigned long)hr);
        return 4;
    }
    probe_log("device ready");
    while (probe_pump())
    {
        int i;
        IDirect3DDevice9_BeginScene(device);
        for (i = 0; i < 4; i++)
        {
            const struct probe_quadrant *q = &probe_quadrants[i];
            D3DRECT rect = {q->left, q->top, q->right, q->bottom};
            IDirect3DDevice9_Clear(device, 1, &rect, D3DCLEAR_TARGET, D3DCOLOR_COLORVALUE(q->r, q->g, q->b, 1.0f),
                                   1.0f, 0);
        }
        IDirect3DDevice9_EndScene(device);
        hr = IDirect3DDevice9_Present(device, NULL, NULL, NULL, NULL);
        if (FAILED(hr))
        {
            probe_log("Present failed: 0x%08lx", (unsigned long)hr);
            break;
        }
        probe_frame_done(++frame);
    }
    IDirect3DDevice9_Release(device);
    IDirect3D9_Release(d3d);
    probe_log("exit");
    return 0;
}
