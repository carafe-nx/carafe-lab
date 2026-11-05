#define COBJMACROS
#include "probe.h"

#include <d3d9.h>
#include <string.h>

#define CELL 160
#define COLUMNS 8
#define SIDE 8

struct probe_texture_format
{
    D3DFORMAT format;
    const char *name;
    int block_bytes;
    int texel_bytes;
};

static const struct probe_texture_format probe_formats[] =
{
    {D3DFMT_A8R8G8B8, "A8R8G8B8", 0, 4},
    {D3DFMT_X8R8G8B8, "X8R8G8B8", 0, 4},
    {D3DFMT_A8B8G8R8, "A8B8G8R8", 0, 4},
    {D3DFMT_R5G6B5, "R5G6B5", 0, 2},
    {D3DFMT_X1R5G5B5, "X1R5G5B5", 0, 2},
    {D3DFMT_A1R5G5B5, "A1R5G5B5", 0, 2},
    {D3DFMT_A4R4G4B4, "A4R4G4B4", 0, 2},
    {D3DFMT_DXT1, "DXT1", 8, 0},
    {D3DFMT_DXT3, "DXT3", 16, 0},
    {D3DFMT_DXT5, "DXT5", 16, 0},
};

#define FORMAT_COUNT (sizeof(probe_formats) / sizeof(probe_formats[0]))

static const BYTE probe_colors[4][3] =
{
    {255, 0, 0},
    {0, 255, 0},
    {0, 0, 255},
    {255, 255, 255},
};

static const char *const probe_color_names[4] = {"red", "green", "blue", "white"};

struct probe_vertex
{
    float x, y, z, rhw;
    float u, v;
};

static WORD probe_565(const BYTE *c)
{
    return (WORD)(((c[0] >> 3) << 11) | ((c[1] >> 2) << 5) | (c[2] >> 3));
}

static DWORD probe_texel(D3DFORMAT format, const BYTE *c)
{
    switch (format)
    {
    case D3DFMT_A8R8G8B8:
    case D3DFMT_X8R8G8B8:
        return 0xff000000u | ((DWORD)c[0] << 16) | ((DWORD)c[1] << 8) | c[2];
    case D3DFMT_A8B8G8R8:
        return 0xff000000u | ((DWORD)c[2] << 16) | ((DWORD)c[1] << 8) | c[0];
    case D3DFMT_R5G6B5:
        return probe_565(c);
    case D3DFMT_X1R5G5B5:
    case D3DFMT_A1R5G5B5:
        return 0x8000u | ((DWORD)(c[0] >> 3) << 10) | ((DWORD)(c[1] >> 3) << 5) | (DWORD)(c[2] >> 3);
    case D3DFMT_A4R4G4B4:
        return 0xf000u | ((DWORD)(c[0] >> 4) << 8) | ((DWORD)(c[1] >> 4) << 4) | (DWORD)(c[2] >> 4);
    default:
        return 0;
    }
}

static void probe_block(D3DFORMAT format, const BYTE *c, BYTE *out)
{
    BYTE *color = out;
    WORD c565 = probe_565(c);

    if (format == D3DFMT_DXT3)
    {
        memset(out, 0xff, 8);
        color = out + 8;
    }
    else if (format == D3DFMT_DXT5)
    {
        out[0] = 0xff;
        out[1] = 0xff;
        memset(out + 2, 0, 6);
        color = out + 8;
    }
    color[0] = (BYTE)(c565 & 0xff);
    color[1] = (BYTE)(c565 >> 8);
    color[2] = 0;
    color[3] = 0;
    memset(color + 4, 0, 4);
}

static void probe_fill(const struct probe_texture_format *f, const D3DLOCKED_RECT *locked)
{
    BYTE *bits = locked->pBits;
    int x, y;

    if (f->block_bytes)
    {
        for (y = 0; y < SIDE / 4; y++)
            for (x = 0; x < SIDE / 4; x++)
                probe_block(f->format, probe_colors[y * 2 + x], bits + y * locked->Pitch + x * f->block_bytes);
        return;
    }
    for (y = 0; y < SIDE; y++)
    {
        for (x = 0; x < SIDE; x++)
        {
            DWORD texel = probe_texel(f->format, probe_colors[(y / 4) * 2 + x / 4]);
            BYTE *at = bits + y * locked->Pitch + x * f->texel_bytes;
            if (f->texel_bytes == 4)
                memcpy(at, &texel, 4);
            else
            {
                WORD half = (WORD)texel;
                memcpy(at, &half, 2);
            }
        }
    }
}

static IDirect3DTexture9 *probe_make(IDirect3DDevice9 *device, const struct probe_texture_format *f, BOOL staged)
{
    IDirect3DTexture9 *source = NULL, *target = NULL;
    D3DLOCKED_RECT locked;
    HRESULT hr;

    hr = IDirect3DDevice9_CreateTexture(device, SIDE, SIDE, 1, 0, f->format,
                                        staged ? D3DPOOL_SYSTEMMEM : D3DPOOL_MANAGED, &source, NULL);
    if (FAILED(hr))
    {
        probe_log("texture %s %s: CreateTexture failed 0x%08lx", f->name, staged ? "default" : "managed",
                  (unsigned long)hr);
        return NULL;
    }
    hr = IDirect3DTexture9_LockRect(source, 0, &locked, NULL, 0);
    if (FAILED(hr))
    {
        probe_log("texture %s: LockRect failed 0x%08lx", f->name, (unsigned long)hr);
        IDirect3DTexture9_Release(source);
        return NULL;
    }
    probe_fill(f, &locked);
    IDirect3DTexture9_UnlockRect(source, 0);
    if (!staged)
        return source;
    hr = IDirect3DDevice9_CreateTexture(device, SIDE, SIDE, 1, 0, f->format, D3DPOOL_DEFAULT, &target, NULL);
    if (SUCCEEDED(hr))
        hr = IDirect3DDevice9_UpdateTexture(device, (IDirect3DBaseTexture9 *)source, (IDirect3DBaseTexture9 *)target);
    IDirect3DTexture9_Release(source);
    if (FAILED(hr))
    {
        probe_log("texture %s default: UpdateTexture failed 0x%08lx", f->name, (unsigned long)hr);
        if (target)
            IDirect3DTexture9_Release(target);
        return NULL;
    }
    return target;
}

static void probe_cell(int index, LONG *left, LONG *top)
{
    *left = (index % COLUMNS) * CELL;
    *top = (index / COLUMNS) * CELL;
}

static void probe_draw(IDirect3DDevice9 *device, IDirect3DTexture9 *texture, int index)
{
    struct probe_vertex quad[4];
    LONG left, top;
    float l, t, r, b;

    probe_cell(index, &left, &top);
    l = (float)left + 8.0f - 0.5f;
    t = (float)top + 8.0f - 0.5f;
    r = (float)(left + CELL) - 8.0f - 0.5f;
    b = (float)(top + CELL) - 8.0f - 0.5f;
    quad[0] = (struct probe_vertex){l, t, 0.0f, 1.0f, 0.0f, 0.0f};
    quad[1] = (struct probe_vertex){r, t, 0.0f, 1.0f, 1.0f, 0.0f};
    quad[2] = (struct probe_vertex){l, b, 0.0f, 1.0f, 0.0f, 1.0f};
    quad[3] = (struct probe_vertex){r, b, 0.0f, 1.0f, 1.0f, 1.0f};
    IDirect3DDevice9_SetTexture(device, 0, (IDirect3DBaseTexture9 *)texture);
    IDirect3DDevice9_DrawPrimitiveUP(device, D3DPT_TRIANGLESTRIP, 2, quad, sizeof(quad[0]));
}

static void probe_check(IDirect3DDevice9 *device, IDirect3DTexture9 **textures)
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
    for (i = 0; i < FORMAT_COUNT * 2; i++)
    {
        const struct probe_texture_format *f = &probe_formats[i / 2];
        const char *pool = (i % 2) ? "default" : "managed";
        char result[256];
        int length = 0, wrong = 0, q;
        LONG left, top;

        if (!textures[i])
            continue;
        probe_cell((int)i, &left, &top);
        for (q = 0; q < 4; q++)
        {
            LONG x = left + 8 + (CELL - 16) / 4 + (q % 2) * (CELL - 16) / 2;
            LONG y = top + 8 + (CELL - 16) / 4 + (q / 2) * (CELL - 16) / 2;
            DWORD pixel = *(const DWORD *)((const BYTE *)locked.pBits + y * locked.Pitch + x * 4);
            int r = (int)((pixel >> 16) & 0xff), g = (int)((pixel >> 8) & 0xff), b = (int)(pixel & 0xff);
            int dr = r - probe_colors[q][0], dg = g - probe_colors[q][1], db = b - probe_colors[q][2];
            if (dr < -40 || dr > 40 || dg < -40 || dg > 40 || db < -40 || db > 40)
            {
                wrong = 1;
                length += snprintf(result + length, sizeof(result) - (size_t)length, " %s→%d,%d,%d",
                                   probe_color_names[q], r, g, b);
            }
        }
        if (wrong)
        {
            failed++;
            probe_log("texture %s %s: wrong%s", f->name, pool, result);
        }
        else
            probe_log("texture %s %s: ok", f->name, pool);
    }
    probe_log("textures checked: %u wrong", failed);
    IDirect3DSurface9_UnlockRect(copy);
done:
    if (copy)
        IDirect3DSurface9_Release(copy);
    if (back)
        IDirect3DSurface9_Release(back);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    IDirect3D9 *d3d;
    IDirect3DDevice9 *device = NULL;
    D3DPRESENT_PARAMETERS params = {0};
    IDirect3DTexture9 *textures[FORMAT_COUNT * 2] = {0};
    unsigned int frame = 0, i;
    HRESULT hr;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("textures");
    window = probe_window("Carafe texture formats probe", probe_default_proc);
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
    for (i = 0; i < FORMAT_COUNT * 2; i++)
    {
        const struct probe_texture_format *f = &probe_formats[i / 2];
        if (FAILED(IDirect3D9_CheckDeviceFormat(d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, D3DFMT_X8R8G8B8, 0,
                                                D3DRTYPE_TEXTURE, f->format)))
        {
            if (i % 2 == 0)
                probe_log("texture %s: not supported", f->name);
            continue;
        }
        textures[i] = probe_make(device, f, (BOOL)(i % 2));
    }
    IDirect3DDevice9_SetFVF(device, D3DFVF_XYZRHW | D3DFVF_TEX1);
    IDirect3DDevice9_SetRenderState(device, D3DRS_LIGHTING, FALSE);
    IDirect3DDevice9_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_NONE);
    IDirect3DDevice9_SetRenderState(device, D3DRS_ZENABLE, D3DZB_FALSE);
    IDirect3DDevice9_SetRenderState(device, D3DRS_ALPHABLENDENABLE, FALSE);
    IDirect3DDevice9_SetRenderState(device, D3DRS_ALPHATESTENABLE, FALSE);
    IDirect3DDevice9_SetTextureStageState(device, 0, D3DTSS_COLOROP, D3DTOP_SELECTARG1);
    IDirect3DDevice9_SetTextureStageState(device, 0, D3DTSS_COLORARG1, D3DTA_TEXTURE);
    IDirect3DDevice9_SetTextureStageState(device, 0, D3DTSS_ALPHAOP, D3DTOP_SELECTARG1);
    IDirect3DDevice9_SetTextureStageState(device, 0, D3DTSS_ALPHAARG1, D3DTA_TEXTURE);
    IDirect3DDevice9_SetSamplerState(device, 0, D3DSAMP_MINFILTER, D3DTEXF_POINT);
    IDirect3DDevice9_SetSamplerState(device, 0, D3DSAMP_MAGFILTER, D3DTEXF_POINT);
    IDirect3DDevice9_SetSamplerState(device, 0, D3DSAMP_MIPFILTER, D3DTEXF_NONE);
    probe_log("device ready, %u formats", (unsigned int)FORMAT_COUNT);
    while (probe_pump())
    {
        IDirect3DDevice9_Clear(device, 0, NULL, D3DCLEAR_TARGET, D3DCOLOR_XRGB(40, 40, 40), 1.0f, 0);
        IDirect3DDevice9_BeginScene(device);
        for (i = 0; i < FORMAT_COUNT * 2; i++)
            if (textures[i])
                probe_draw(device, textures[i], (int)i);
        IDirect3DDevice9_EndScene(device);
        if (frame == 60)
            probe_check(device, textures);
        hr = IDirect3DDevice9_Present(device, NULL, NULL, NULL, NULL);
        if (FAILED(hr))
        {
            probe_log("Present failed: 0x%08lx", (unsigned long)hr);
            break;
        }
        probe_frame_done(++frame);
    }
    for (i = 0; i < FORMAT_COUNT * 2; i++)
        if (textures[i])
            IDirect3DTexture9_Release(textures[i]);
    IDirect3DDevice9_Release(device);
    IDirect3D9_Release(d3d);
    probe_log("exit");
    return 0;
}
