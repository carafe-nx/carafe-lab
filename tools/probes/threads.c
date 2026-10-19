#define COBJMACROS
#include "probe.h"

#include <d3d9.h>

#define PROBE_MAX_THREADS 512
#define PROBE_CHURN 300
#define PROBE_STACK (64 * 1024)

static HANDLE hold_event;
static volatile LONG hold_started;
static HANDLE held[PROBE_MAX_THREADS];
static unsigned int held_count;
static HANDLE kept[PROBE_CHURN];
static DWORD hold_release_ms = INFINITE;
static HANDLE go_event;

static DWORD WINAPI hold_thread(void *arg)
{
    HANDLE events[2];

    InterlockedIncrement(&hold_started);
    if (((UINT_PTR)arg & 1) && hold_release_ms != INFINITE)
    {
        events[0] = hold_event;
        events[1] = go_event;
        if (WaitForMultipleObjects(2, events, FALSE, INFINITE) == WAIT_OBJECT_0 + 1)
            Sleep(hold_release_ms);
        return 0;
    }
    WaitForSingleObject(hold_event, INFINITE);
    return 0;
}

static DWORD WINAPI quick_thread(void *arg)
{
    (void)arg;
    return 0;
}

static HANDLE start_thread(LPTHREAD_START_ROUTINE proc, UINT_PTR index)
{
    return CreateThread(NULL, PROBE_STACK, proc, (void *)index, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
}

static unsigned int hold_all(const char *phase)
{
    DWORD error = 0;
    unsigned int waited;

    ResetEvent(hold_event);
    hold_started = 0;
    held_count = 0;
    while (held_count < PROBE_MAX_THREADS)
    {
        HANDLE thread = start_thread(hold_thread, held_count);

        if (!thread)
        {
            error = GetLastError();
            break;
        }
        held[held_count++] = thread;
    }
    for (waited = 0; (unsigned int)hold_started < held_count && waited < 1000; waited++)
        Sleep(10);
    probe_log("ceiling %s: %u threads, started %ld, refused with error %lu", phase, held_count, hold_started,
              error);
    return held_count;
}

static void release_all(void)
{
    unsigned int i;
    unsigned int stuck = 0;
    unsigned int count = held_count;
    DWORD start = GetTickCount();

    SetEvent(hold_event);
    for (i = 0; i < held_count; i++)
    {
        if (WaitForSingleObject(held[i], 10000) != WAIT_OBJECT_0)
            stuck++;
        CloseHandle(held[i]);
    }
    if (stuck)
        probe_log("release: %u threads did not end in 10 s", stuck);
    if (count)
        probe_log("released %u threads in %lu ms", count, GetTickCount() - start);
    held_count = 0;
}

static void churn_joined(void)
{
    unsigned int i;
    unsigned int refused = 0;

    for (i = 0; i < PROBE_CHURN; i++)
    {
        HANDLE thread = start_thread(quick_thread, 0);

        if (!thread)
        {
            refused++;
            continue;
        }
        WaitForSingleObject(thread, 10000);
        CloseHandle(thread);
    }
    probe_log("churn joined: %u threads, %u refused", PROBE_CHURN, refused);
}

static void churn_detached(void)
{
    unsigned int i;
    unsigned int refused = 0;
    DWORD error = 0;

    for (i = 0; i < PROBE_CHURN; i++)
    {
        HANDLE thread = start_thread(quick_thread, 0);

        if (!thread)
        {
            refused++;
            error = GetLastError();
            Sleep(10);
            continue;
        }
        CloseHandle(thread);
        if (i % 10 == 9)
            Sleep(1);
    }
    probe_log("churn detached: %u threads, %u refused, last error %lu", PROBE_CHURN, refused, error);
}

static unsigned int churn_kept(void)
{
    unsigned int i;
    unsigned int count = 0;
    unsigned int refused = 0;

    for (i = 0; i < PROBE_CHURN; i++)
    {
        HANDLE thread = start_thread(quick_thread, 0);

        if (!thread)
        {
            refused++;
            continue;
        }
        WaitForSingleObject(thread, 10000);
        kept[count++] = thread;
    }
    probe_log("churn with open handles: %u threads, %u refused", PROBE_CHURN, refused);
    return count;
}

static IDirect3DDevice9 *create_device(IDirect3D9 *d3d, HWND window, HRESULT *hr)
{
    D3DPRESENT_PARAMETERS params = {0};
    IDirect3DDevice9 *device = NULL;

    params.Windowed = TRUE;
    params.SwapEffect = D3DSWAPEFFECT_DISCARD;
    params.BackBufferFormat = D3DFMT_X8R8G8B8;
    params.BackBufferWidth = PROBE_WIDTH;
    params.BackBufferHeight = PROBE_HEIGHT;
    params.hDeviceWindow = window;
    params.PresentationInterval = D3DPRESENT_INTERVAL_ONE;
    *hr = IDirect3D9_CreateDevice(d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
                                  D3DCREATE_HARDWARE_VERTEXPROCESSING, &params, &device);
    return SUCCEEDED(*hr) ? device : NULL;
}

static BOOL present(IDirect3DDevice9 *device, DWORD milliseconds)
{
    DWORD start = GetTickCount();
    unsigned int frame;

    for (frame = 1; GetTickCount() - start < milliseconds && probe_pump(); frame++)
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
        if (FAILED(IDirect3DDevice9_Present(device, NULL, NULL, NULL, NULL)))
        {
            probe_log("Present failed");
            return FALSE;
        }
        probe_frame_done(frame);
    }
    return TRUE;
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    IDirect3D9 *d3d;
    IDirect3DDevice9 *device;
    IDirect3DDevice9 *second;
    HRESULT hr;
    unsigned int count;
    unsigned int i;

    (void)instance;
    (void)previous;
    (void)show;
    probe_open_log("threads");
    hold_event = CreateEventA(NULL, TRUE, FALSE, NULL);
    go_event = CreateEventA(NULL, TRUE, FALSE, NULL);
    window = probe_window("Carafe threads probe", probe_default_proc);
    probe_pump();
    if (command_line && strstr(command_line, "census"))
    {
        probe_log("census: no ceiling or churn phases");
        goto device;
    }

    hold_all("at start");
    release_all();
    hold_all("right after release");
    release_all();
    Sleep(3000);
    hold_all("3 s after release");
    release_all();

    churn_joined();
    hold_all("after joined churn");
    release_all();

    churn_detached();
    Sleep(3000);
    hold_all("after detached churn");
    release_all();

    count = churn_kept();
    hold_all("with open handles of ended threads");
    release_all();
    for (i = 0; i < count; i++)
        CloseHandle(kept[i]);
    Sleep(3000);
    hold_all("after closing those handles");
    release_all();

device:
    d3d = Direct3DCreate9(D3D_SDK_VERSION);
    if (!d3d)
    {
        probe_log("Direct3DCreate9 failed");
        return 3;
    }
    device = create_device(d3d, window, &hr);
    if (!device)
    {
        probe_log("CreateDevice failed: 0x%08lx", (unsigned long)hr);
        return 4;
    }
    probe_log("device ready");
    probe_log("idle for 15 s");
    present(device, 15000);
    second = NULL;
    if (command_line && strstr(command_line, "dxvk-hard"))
    {
        hold_all("with a Direct3D 9 device");
        probe_log("second device with every thread taken");
        second = create_device(d3d, window, &hr);
        probe_log("second device survived: 0x%08lx", (unsigned long)hr);
    }
    else if (command_line && strstr(command_line, "dxvk"))
    {
        hold_release_ms = 2000;
        hold_all("with a Direct3D 9 device, half of them ending 2 s after the device is asked for");
        SetEvent(go_event);
        probe_log("second device with every thread taken");
        second = create_device(d3d, window, &hr);
        probe_log("second device survived: 0x%08lx", (unsigned long)hr);
    }
    else
    {
        hold_all("with a Direct3D 9 device");
        probe_log("holding every thread for 30 s");
        present(device, 30000);
    }
    release_all();
    if (second)
        IDirect3DDevice9_Release(second);
    present(device, 5000);
    IDirect3DDevice9_Release(device);
    IDirect3D9_Release(d3d);
    probe_log("exit");
    return 0;
}
