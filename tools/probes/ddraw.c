#define COBJMACROS
#include <windows.h>
#include <ddraw.h>
#include <stdio.h>
#include <string.h>

#define WIDTH 1280
#define HEIGHT 720

static FILE *log_file;

static void record(const char *operation, HRESULT result)
{
    if (!log_file) return;
    fprintf(log_file, "%s: %08lx\n", operation, (unsigned long)result);
    fflush(log_file);
}

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_LBUTTONDOWN)
    {
        record("mouse button received", S_OK);
        return 0;
    }
    if (message == WM_DESTROY)
    {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcA(window, message, wparam, lparam);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command, int show)
{
    WNDCLASSA klass = {0};
    LPDIRECTDRAW draw = NULL;
    LPDIRECTDRAWSURFACE primary = NULL, picture = NULL;
    DDSURFACEDESC desc = {0};
    HWND window;
    HRESULT result;
    MSG message;
    int exit_code = 1;
    DWORD bits = command && strstr(command, "--rgb565") != NULL ? 16 : 32;

    (void)previous;
    (void)show;
    log_file = fopen("C:\\ddraw-input-idle.log", "w");
    if (command && strstr(command, "--no-publication") != NULL)
    {
        BOOL cleared = SetEnvironmentVariableA("WINE_NX_WINDOW_PICTURE_EVENT", NULL);
        record("Disable publication notification", cleared ? S_OK : HRESULT_FROM_WIN32(GetLastError()));
        if (!cleared) goto done;
    }
    klass.lpfnWndProc = window_proc;
    klass.hInstance = instance;
    klass.hCursor = LoadCursor(NULL, IDC_ARROW);
    klass.lpszClassName = "CarafeDirectDrawInputIdle";
    if (!RegisterClassA(&klass)) goto done;
    window = CreateWindowExA(0, klass.lpszClassName, "DirectDraw input-idle contract",
                            WS_POPUP | WS_VISIBLE, 0, 0, WIDTH, HEIGHT, NULL, NULL, instance, NULL);
    if (!window) goto done;

    result = DirectDrawCreate(NULL, &draw, NULL);
    record("DirectDrawCreate", result);
    if (FAILED(result)) goto done;
    result = IDirectDraw_SetCooperativeLevel(draw, window, DDSCL_EXCLUSIVE | DDSCL_FULLSCREEN);
    record("SetCooperativeLevel", result);
    if (FAILED(result)) goto done;
    result = IDirectDraw_SetDisplayMode(draw, WIDTH, HEIGHT, bits);
    record("SetDisplayMode", result);
    if (FAILED(result)) goto done;

    desc.dwSize = sizeof(desc);
    desc.dwFlags = DDSD_CAPS;
    desc.ddsCaps.dwCaps = DDSCAPS_PRIMARYSURFACE;
    result = IDirectDraw_CreateSurface(draw, &desc, &primary, NULL);
    record("Create primary", result);
    if (FAILED(result)) goto done;
    desc.dwFlags = DDSD_CAPS | DDSD_WIDTH | DDSD_HEIGHT;
    desc.ddsCaps.dwCaps = DDSCAPS_OFFSCREENPLAIN | DDSCAPS_SYSTEMMEMORY;
    desc.dwWidth = WIDTH;
    desc.dwHeight = HEIGHT;
    result = IDirectDraw_CreateSurface(draw, &desc, &picture, NULL);
    record("Create picture", result);
    if (FAILED(result)) goto done;

    desc.dwSize = sizeof(desc);
    result = IDirectDrawSurface_Lock(picture, NULL, &desc, DDLOCK_WAIT | DDLOCK_WRITEONLY, NULL);
    record("Lock", result);
    if (FAILED(result)) goto done;
    if (desc.ddpfPixelFormat.dwRGBBitCount != bits)
    {
        record("Unexpected pixel format", E_FAIL);
        IDirectDrawSurface_Unlock(picture, NULL);
        goto done;
    }
    for (DWORD y = 0; y < HEIGHT; y++)
    {
        BYTE *row = (BYTE *)desc.lpSurface + y * desc.lPitch;
        for (DWORD x = 0; x < WIDTH; x++)
        {
            DWORD color = ((x / 80 + y / 80) & 1) ? 0x0033cc55 : 0x00cc3355;
            if (bits == 16)
                ((WORD *)row)[x] = (WORD)(((color >> 19) << 11) | (((color >> 10) & 63) << 5)
                                        | ((color >> 3) & 31));
            else
                ((DWORD *)row)[x] = color;
        }
    }
    result = IDirectDrawSurface_Unlock(picture, NULL);
    record("Unlock", result);
    if (FAILED(result)) goto done;
    result = IDirectDrawSurface_Blt(primary, NULL, picture, NULL, DDBLT_WAIT, NULL);
    record("Publish picture", result);
    if (FAILED(result)) goto done;
    record("Entering GetMessage", S_OK);
    while (GetMessageA(&message, NULL, 0, 0) > 0)
    {
        TranslateMessage(&message);
        DispatchMessageA(&message);
    }
    exit_code = 0;

done:
    if (picture) IDirectDrawSurface_Release(picture);
    if (primary) IDirectDrawSurface_Release(primary);
    if (draw) IDirectDraw_Release(draw);
    if (log_file) fclose(log_file);
    return exit_code;
}
