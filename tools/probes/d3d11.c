#define COBJMACROS
#include "probe.h"

#include <d3d11_1.h>

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    DXGI_SWAP_CHAIN_DESC desc = {0};
    IDXGISwapChain *swapchain = NULL;
    ID3D11Device *device = NULL;
    ID3D11DeviceContext *context = NULL;
    ID3D11DeviceContext1 *context1 = NULL;
    ID3D11Texture2D *back = NULL;
    ID3D11RenderTargetView *target = NULL;
    D3D_FEATURE_LEVEL level;
    HRESULT hr;
    unsigned int frame = 0;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("d3d11");
    window = probe_window("Carafe D3D11 probe", probe_default_proc);
    desc.BufferDesc.Width = PROBE_WIDTH;
    desc.BufferDesc.Height = PROBE_HEIGHT;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 2;
    desc.OutputWindow = window;
    desc.Windowed = TRUE;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    hr = D3D11CreateDeviceAndSwapChain(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0, D3D11_SDK_VERSION, &desc,
                                       &swapchain, &device, &level, &context);
    if (FAILED(hr))
    {
        probe_log("D3D11CreateDeviceAndSwapChain failed: 0x%08lx", (unsigned long)hr);
        return 3;
    }
    probe_log("device ready, feature level 0x%x", (unsigned int)level);
    hr = ID3D11DeviceContext_QueryInterface(context, &IID_ID3D11DeviceContext1, (void **)&context1);
    if (FAILED(hr))
    {
        probe_log("no ID3D11DeviceContext1: 0x%08lx", (unsigned long)hr);
        return 4;
    }
    IDXGISwapChain_GetBuffer(swapchain, 0, &IID_ID3D11Texture2D, (void **)&back);
    ID3D11Device_CreateRenderTargetView(device, (ID3D11Resource *)back, NULL, &target);
    ID3D11Texture2D_Release(back);
    while (probe_pump())
    {
        int i;
        for (i = 0; i < 4; i++)
        {
            const struct probe_quadrant *q = &probe_quadrants[i];
            D3D11_RECT rect = {q->left, q->top, q->right, q->bottom};
            float color[4] = {q->r, q->g, q->b, 1.0f};
            ID3D11DeviceContext1_ClearView(context1, (ID3D11View *)target, color, &rect, 1);
        }
        hr = IDXGISwapChain_Present(swapchain, 1, 0);
        if (FAILED(hr))
        {
            probe_log("Present failed: 0x%08lx", (unsigned long)hr);
            break;
        }
        probe_frame_done(++frame);
    }
    ID3D11RenderTargetView_Release(target);
    ID3D11DeviceContext1_Release(context1);
    ID3D11DeviceContext_Release(context);
    ID3D11Device_Release(device);
    IDXGISwapChain_Release(swapchain);
    probe_log("exit");
    return 0;
}
