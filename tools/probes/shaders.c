#define COBJMACROS
#include <d3d9.h>

#include "probe.h"

#define END 0x0000FFFFu
#define DCL(usage, reg) 0x0000001Fu, 0x80000000u | (usage), 0x900F0000u | (reg)
#define MOV 0x00000001u
#define MUL 0x00000005u
#define DP4 0x00000009u
#define POS(mask) (0xC0000000u | ((DWORD)(mask) << 16))
#define DIFFUSE 0xD00F0000u
#define ADDRESS_X 0xB0010000u
#define TEMP3 0x800F0003u
#define TEMP3_X 0x80000003u
#define INPUT0 0x90E40000u
#define INPUT1_ZYXW 0x90C60001u
#define C_REG(n) (0xA0E40000u | (n))
#define CONST_X(n) (0xA0000000u | (n))
#define CONST_Z(n) (0xA0AA0000u | (n))
#define C_REL(n) (0xA0E42000u | (n))

static const DWORD direct_shader[] = {
    0xFFFE0101u, DCL(0, 0), DCL(2, 1),
    DP4, POS(1), INPUT0, C_REG(4),
    DP4, POS(2), INPUT0, C_REG(5),
    DP4, POS(4), INPUT0, C_REG(6),
    DP4, POS(8), INPUT0, C_REG(7),
    MOV, DIFFUSE, C_REG(1),
    END,
};

static const DWORD constant_index_shader[] = {
    0xFFFE0101u, DCL(0, 0), DCL(2, 1),
    MOV, ADDRESS_X, CONST_X(0),
    DP4, POS(1), INPUT0, C_REL(0),
    DP4, POS(2), INPUT0, C_REL(1),
    DP4, POS(4), INPUT0, C_REL(2),
    DP4, POS(8), INPUT0, C_REL(3),
    MOV, DIFFUSE, C_REG(1),
    END,
};

static const DWORD vertex_index_shader[] = {
    0xFFFE0101u, DCL(0, 0), DCL(2, 1),
    MUL, TEMP3, INPUT1_ZYXW, CONST_Z(2),
    MOV, ADDRESS_X, TEMP3_X,
    DP4, POS(1), INPUT0, C_REL(0),
    DP4, POS(2), INPUT0, C_REL(1),
    DP4, POS(4), INPUT0, C_REL(2),
    DP4, POS(8), INPUT0, C_REL(3),
    MOV, DIFFUSE, C_REG(1),
    END,
};

struct probe_vertex
{
    float x, y, z;
    D3DCOLOR index;
};

struct probe_case
{
    const char *name;
    int shader;
    int base;
    float x, y;
    BYTE color[3];
};

static const struct probe_case probe_cases[] = {
    {"direct c4", 0, 4, -0.66f, 0.5f, {255, 0, 0}},
    {"a0 from a constant, c8", 1, 8, -0.22f, 0.5f, {0, 255, 0}},
    {"a0 from a constant, c40", 1, 40, 0.22f, 0.5f, {0, 0, 255}},
    {"a0 from a constant, c92", 1, 92, 0.66f, 0.5f, {255, 255, 255}},
    {"a0 from a D3DCOLOR vertex, c16", 2, 16, -0.66f, -0.5f, {255, 0, 0}},
    {"a0 from a D3DCOLOR vertex, c48", 2, 48, -0.22f, -0.5f, {0, 255, 0}},
    {"a0 from a D3DCOLOR vertex, c84", 2, 84, 0.22f, -0.5f, {0, 0, 255}},
    {"a0 from a D3DCOLOR vertex, c92", 2, 92, 0.66f, -0.5f, {255, 255, 255}},
};

#define CASE_COUNT (sizeof(probe_cases) / sizeof(probe_cases[0]))

static void probe_draw_case(IDirect3DDevice9 *device, IDirect3DVertexShader9 **shaders, const struct probe_case *c)
{
    const float h = 0.12f;
    const float color[4] = {c->color[0] / 255.0f, c->color[1] / 255.0f, c->color[2] / 255.0f, 1.0f};
    const float base[4] = {(float)c->base, 0.0f, 0.0f, 0.0f};
    const float scale[4] = {0.0f, 0.0f, 255.001f, 0.0f};
    const float matrix[16] = {
        1.0f, 0.0f, 0.0f, c->x,
        0.0f, 1.0f, 0.0f, c->y,
        0.0f, 0.0f, 1.0f, 0.0f,
        0.0f, 0.0f, 0.0f, 1.0f,
    };
    const float zero[16] = {0};
    D3DCOLOR index = D3DCOLOR_ARGB(255, 0, 0, (BYTE)c->base);
    struct probe_vertex quad[4] = {
        {-h, -h, 0.5f, index}, {-h, h, 0.5f, index}, {h, -h, 0.5f, index}, {h, h, 0.5f, index},
    };
    int i;

    for (i = 4; i < 96; i += 4)
        IDirect3DDevice9_SetVertexShaderConstantF(device, (UINT)i, zero, 4);
    IDirect3DDevice9_SetVertexShaderConstantF(device, 0, base, 1);
    IDirect3DDevice9_SetVertexShaderConstantF(device, 1, color, 1);
    IDirect3DDevice9_SetVertexShaderConstantF(device, 2, scale, 1);
    IDirect3DDevice9_SetVertexShaderConstantF(device, (UINT)c->base, matrix, 4);
    IDirect3DDevice9_SetVertexShader(device, shaders[c->shader]);
    IDirect3DDevice9_DrawPrimitiveUP(device, D3DPT_TRIANGLESTRIP, 2, quad, sizeof(quad[0]));
}

static void probe_check(IDirect3DDevice9 *device)
{
    IDirect3DSurface9 *back = NULL, *copy = NULL;
    D3DLOCKED_RECT locked;
    unsigned int i, failed = 0;
    HRESULT hr;

    hr = IDirect3DDevice9_GetBackBuffer(device, 0, 0, D3DBACKBUFFER_TYPE_MONO, &back);
    if (SUCCEEDED(hr))
        hr = IDirect3DDevice9_CreateOffscreenPlainSurface(device, PROBE_WIDTH, PROBE_HEIGHT, D3DFMT_X8R8G8B8,
                                                          D3DPOOL_SYSTEMMEM, &copy, NULL);
    if (SUCCEEDED(hr))
        hr = IDirect3DDevice9_GetRenderTargetData(device, back, copy);
    if (SUCCEEDED(hr))
        hr = IDirect3DSurface9_LockRect(copy, &locked, NULL, D3DLOCK_READONLY);
    if (FAILED(hr))
    {
        probe_log("readback failed 0x%08lx", (unsigned long)hr);
        goto done;
    }
    for (i = 0; i < CASE_COUNT; i++)
    {
        const struct probe_case *c = &probe_cases[i];
        LONG x = (LONG)((c->x + 1.0f) * 0.5f * PROBE_WIDTH);
        LONG y = (LONG)((1.0f - c->y) * 0.5f * PROBE_HEIGHT);
        DWORD pixel = *(const DWORD *)((const BYTE *)locked.pBits + y * locked.Pitch + x * 4);
        int r = (int)((pixel >> 16) & 0xff), g = (int)((pixel >> 8) & 0xff), b = (int)(pixel & 0xff);
        int dr = r - c->color[0], dg = g - c->color[1], db = b - c->color[2];

        if (dr < -40 || dr > 40 || dg < -40 || dg > 40 || db < -40 || db > 40)
        {
            failed++;
            probe_log("shader %s: wrong, %d,%d,%d at %ld,%ld", c->name, r, g, b, (long)x, (long)y);
        }
        else
            probe_log("shader %s: ok", c->name);
    }
    probe_log("shaders checked: %u wrong", failed);
    IDirect3DSurface9_UnlockRect(copy);
done:
    if (copy)
        IDirect3DSurface9_Release(copy);
    if (back)
        IDirect3DSurface9_Release(back);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    static const D3DVERTEXELEMENT9 elements[] = {
        {0, 0, D3DDECLTYPE_FLOAT3, D3DDECLMETHOD_DEFAULT, D3DDECLUSAGE_POSITION, 0},
        {0, 12, D3DDECLTYPE_D3DCOLOR, D3DDECLMETHOD_DEFAULT, D3DDECLUSAGE_BLENDINDICES, 0},
        D3DDECL_END(),
    };
    const DWORD *codes[3] = {direct_shader, constant_index_shader, vertex_index_shader};
    IDirect3DVertexShader9 *shaders[3] = {0};
    IDirect3DVertexDeclaration9 *declaration = NULL;
    HWND window;
    IDirect3D9 *d3d;
    IDirect3DDevice9 *device = NULL;
    D3DPRESENT_PARAMETERS params = {0};
    unsigned int frame = 0, i;
    HRESULT hr;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("shaders");
    window = probe_window("Carafe vertex shader probe", probe_default_proc);
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
    for (i = 0; i < 3; i++)
    {
        hr = IDirect3DDevice9_CreateVertexShader(device, codes[i], &shaders[i]);
        if (FAILED(hr))
        {
            probe_log("CreateVertexShader %u failed: 0x%08lx", i, (unsigned long)hr);
            return 5;
        }
    }
    hr = IDirect3DDevice9_CreateVertexDeclaration(device, elements, &declaration);
    if (FAILED(hr))
    {
        probe_log("CreateVertexDeclaration failed: 0x%08lx", (unsigned long)hr);
        return 6;
    }
    IDirect3DDevice9_SetVertexDeclaration(device, declaration);
    IDirect3DDevice9_SetRenderState(device, D3DRS_LIGHTING, FALSE);
    IDirect3DDevice9_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_NONE);
    IDirect3DDevice9_SetRenderState(device, D3DRS_ZENABLE, D3DZB_FALSE);
    IDirect3DDevice9_SetTextureStageState(device, 0, D3DTSS_COLOROP, D3DTOP_SELECTARG1);
    IDirect3DDevice9_SetTextureStageState(device, 0, D3DTSS_COLORARG1, D3DTA_DIFFUSE);
    probe_log("device ready, %u cases", (unsigned int)CASE_COUNT);
    while (probe_pump())
    {
        IDirect3DDevice9_Clear(device, 0, NULL, D3DCLEAR_TARGET, D3DCOLOR_XRGB(40, 40, 40), 1.0f, 0);
        IDirect3DDevice9_BeginScene(device);
        for (i = 0; i < CASE_COUNT; i++)
            probe_draw_case(device, shaders, &probe_cases[i]);
        IDirect3DDevice9_EndScene(device);
        if (frame == 60)
            probe_check(device);
        hr = IDirect3DDevice9_Present(device, NULL, NULL, NULL, NULL);
        if (FAILED(hr))
        {
            probe_log("Present failed: 0x%08lx", (unsigned long)hr);
            break;
        }
        probe_frame_done(++frame);
    }
    for (i = 0; i < 3; i++)
        if (shaders[i])
            IDirect3DVertexShader9_Release(shaders[i]);
    IDirect3DVertexDeclaration9_Release(declaration);
    IDirect3DDevice9_Release(device);
    IDirect3D9_Release(d3d);
    probe_log("exit");
    return 0;
}
