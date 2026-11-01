#include "probe.h"

#define DIRECTINPUT_VERSION 0x0700
#include <dinput.h>
#include <xinput.h>

typedef DWORD (WINAPI *get_state_fn)(DWORD, XINPUT_STATE *);

#define SHOWN_LINES 24

static char shown[SHOWN_LINES][96];
static int shown_count;
static POINT last_cursor;
static BOOL have_cursor;
static LONG move_x, move_y;

static void remember(const char *format, ...)
{
    char line[96];
    va_list args;

    va_start(args, format);
    vsnprintf(line, sizeof(line), format, args);
    va_end(args);
    probe_log("%s", line);
    if (shown_count == SHOWN_LINES)
    {
        memmove(shown[0], shown[1], sizeof(shown[0]) * (SHOWN_LINES - 1));
        shown_count--;
    }
    snprintf(shown[shown_count++], sizeof(shown[0]), "%s", line);
}

static const char *mouse_button(UINT message, WPARAM wparam)
{
    switch (message)
    {
    case WM_LBUTTONDOWN: case WM_LBUTTONUP: return "left";
    case WM_RBUTTONDOWN: case WM_RBUTTONUP: return "right";
    case WM_MBUTTONDOWN: case WM_MBUTTONUP: return "middle";
    default: return GET_XBUTTON_WPARAM(wparam) == XBUTTON1 ? "x1" : "x2";
    }
}

static LRESULT CALLBACK input_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    switch (message)
    {
    case WM_KEYDOWN:
    case WM_SYSKEYDOWN:
        if (!(lparam & (1 << 30)))
            remember("key down 0x%02x", (unsigned int)wparam);
        InvalidateRect(window, NULL, FALSE);
        return 0;
    case WM_KEYUP:
    case WM_SYSKEYUP:
        remember("key up 0x%02x", (unsigned int)wparam);
        InvalidateRect(window, NULL, FALSE);
        return 0;
    case WM_LBUTTONDOWN: case WM_RBUTTONDOWN: case WM_MBUTTONDOWN: case WM_XBUTTONDOWN:
        remember("mouse down %s", mouse_button(message, wparam));
        InvalidateRect(window, NULL, FALSE);
        return message == WM_XBUTTONDOWN ? TRUE : 0;
    case WM_LBUTTONUP: case WM_RBUTTONUP: case WM_MBUTTONUP: case WM_XBUTTONUP:
        remember("mouse up %s", mouse_button(message, wparam));
        InvalidateRect(window, NULL, FALSE);
        return message == WM_XBUTTONUP ? TRUE : 0;
    case WM_MOUSEWHEEL:
        remember("wheel %s", GET_WHEEL_DELTA_WPARAM(wparam) > 0 ? "up" : "down");
        InvalidateRect(window, NULL, FALSE);
        return 0;
    case WM_MOUSEHWHEEL:
        remember("wheel %s", GET_WHEEL_DELTA_WPARAM(wparam) > 0 ? "right" : "left");
        InvalidateRect(window, NULL, FALSE);
        return 0;
    case WM_PAINT:
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        RECT all = {0, 0, PROBE_WIDTH, PROBE_HEIGHT};
        int i;

        FillRect(dc, &all, (HBRUSH)GetStockObject(BLACK_BRUSH));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, RGB(230, 230, 230));
        TextOutA(dc, 16, 8, "Carafe input probe", 18);
        for (i = 0; i < shown_count; i++)
            TextOutA(dc, 16, 36 + i * 26, shown[i], (int)strlen(shown[i]));
        EndPaint(window, &paint);
        return 0;
    }
    default:
        return probe_default_proc(window, message, wparam, lparam);
    }
}

static get_state_fn load_xinput(void)
{
    static const char *const names[] = {"xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"};
    unsigned int i;

    for (i = 0; i < sizeof(names) / sizeof(names[0]); i++)
    {
        HMODULE module = LoadLibraryA(names[i]);
        if (module)
        {
            get_state_fn get_state = (get_state_fn)(void *)GetProcAddress(module, "XInputGetState");
            if (get_state)
            {
                probe_log("xinput %s", names[i]);
                return get_state;
            }
        }
    }
    probe_log("xinput missing");
    return NULL;
}

static IDirectInputDeviceA *dinput_mouse;
static IDirectInputDeviceA *dinput_keyboard;
static BYTE dinput_buttons[4];
static BYTE dinput_keys[256];
static LONG dinput_move_x, dinput_move_y;

static IDirectInputDeviceA *open_dinput_device(IDirectInputA *dinput, HWND window, REFGUID guid,
                                               LPCDIDATAFORMAT format, const char *name)
{
    IDirectInputDeviceA *device = NULL;
    HRESULT hr;

    if (FAILED(hr = IDirectInput_CreateDevice(dinput, guid, &device, NULL)))
    {
        probe_log("dinput %s device failed 0x%08lx", name, (unsigned long)hr);
        return NULL;
    }
    IDirectInputDevice_SetDataFormat(device, format);
    IDirectInputDevice_SetCooperativeLevel(device, window, DISCL_NONEXCLUSIVE | DISCL_FOREGROUND);
    hr = IDirectInputDevice_Acquire(device);
    probe_log("dinput %s acquire 0x%08lx", name, (unsigned long)hr);
    return device;
}

static void open_dinput(HWND window)
{
    IDirectInputA *dinput = NULL;
    HRESULT hr = DirectInputCreateA(GetModuleHandleA(NULL), DIRECTINPUT_VERSION, &dinput, NULL);

    if (FAILED(hr))
    {
        probe_log("dinput missing 0x%08lx", (unsigned long)hr);
        return;
    }
    probe_log("dinput 7 ready");
    dinput_mouse = open_dinput_device(dinput, window, &GUID_SysMouse, &c_dfDIMouse, "mouse");
    dinput_keyboard = open_dinput_device(dinput, window, &GUID_SysKeyboard, &c_dfDIKeyboard, "keyboard");
}

static HRESULT read_dinput(IDirectInputDeviceA *device, DWORD size, void *data)
{
    HRESULT hr = IDirectInputDevice_GetDeviceState(device, size, data);

    if (hr == DIERR_INPUTLOST || hr == DIERR_NOTACQUIRED)
    {
        IDirectInputDevice_Acquire(device);
        hr = IDirectInputDevice_GetDeviceState(device, size, data);
    }
    return hr;
}

static void poll_dinput(HWND window)
{
    DIMOUSESTATE mouse;
    BYTE keys[256];
    int i;

    if (dinput_mouse && SUCCEEDED(read_dinput(dinput_mouse, sizeof(mouse), &mouse)))
    {
        dinput_move_x += mouse.lX;
        dinput_move_y += mouse.lY;
        for (i = 0; i < 4; i++)
        {
            if ((mouse.rgbButtons[i] & 0x80) == (dinput_buttons[i] & 0x80))
                continue;
            remember("dinput button %d %s", i, mouse.rgbButtons[i] & 0x80 ? "down" : "up");
            dinput_buttons[i] = mouse.rgbButtons[i];
            InvalidateRect(window, NULL, FALSE);
        }
    }
    if (dinput_keyboard && SUCCEEDED(read_dinput(dinput_keyboard, sizeof(keys), keys)))
    {
        for (i = 0; i < 256; i++)
        {
            if ((keys[i] & 0x80) == (dinput_keys[i] & 0x80))
                continue;
            remember("dinput key 0x%02x %s", i, keys[i] & 0x80 ? "down" : "up");
            dinput_keys[i] = keys[i];
            InvalidateRect(window, NULL, FALSE);
        }
    }
}

static DWORD settle_until;

static void poll_cursor(void)
{
    POINT cursor;

    if (!GetCursorPos(&cursor))
        return;
    if ((LONG)(GetTickCount() - settle_until) < 0)
    {
        have_cursor = FALSE;
        dinput_move_x = dinput_move_y = 0;
        return;
    }
    if (have_cursor)
    {
        move_x += cursor.x - last_cursor.x;
        move_y += cursor.y - last_cursor.y;
    }
    last_cursor = cursor;
    have_cursor = TRUE;
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    get_state_fn get_state;
    XINPUT_STATE state, last;
    BOOL have_last = FALSE;
    DWORD connected = ERROR_DEVICE_NOT_CONNECTED;
    DWORD last_move = GetTickCount();
    DWORD last_paint = GetTickCount();

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("input");
    window = probe_window("Carafe input probe", input_proc);
    get_state = load_xinput();
    open_dinput(window);
    settle_until = GetTickCount() + 3000;
    memset(&last, 0, sizeof(last));
    while (probe_pump())
    {
        if (get_state)
        {
            DWORD result;
            memset(&state, 0, sizeof(state));
            result = get_state(0, &state);
            if (result != connected)
            {
                connected = result;
                remember("pad %s", result == ERROR_SUCCESS ? "connected" : "absent");
            }
            if (result == ERROR_SUCCESS
                && (!have_last || memcmp(&state.Gamepad, &last.Gamepad, sizeof(state.Gamepad)) != 0))
            {
                remember("pad buttons=0x%04x lt=%u rt=%u lx=%d ly=%d rx=%d ry=%d",
                         state.Gamepad.wButtons, state.Gamepad.bLeftTrigger, state.Gamepad.bRightTrigger,
                         state.Gamepad.sThumbLX, state.Gamepad.sThumbLY, state.Gamepad.sThumbRX,
                         state.Gamepad.sThumbRY);
                last = state;
                have_last = TRUE;
                InvalidateRect(window, NULL, FALSE);
            }
        }
        poll_cursor();
        poll_dinput(window);
        if (GetTickCount() - last_paint >= 40)
        {
            InvalidateRect(window, NULL, FALSE);
            last_paint = GetTickCount();
        }
        if (GetTickCount() - last_move >= 100)
        {
            if (move_x || move_y)
            {
                remember("move %ld %ld", move_x, move_y);
                move_x = move_y = 0;
                InvalidateRect(window, NULL, FALSE);
            }
            if (dinput_move_x || dinput_move_y)
            {
                remember("dinput move %ld %ld", dinput_move_x, dinput_move_y);
                dinput_move_x = dinput_move_y = 0;
                InvalidateRect(window, NULL, FALSE);
            }
            last_move = GetTickCount();
        }
        Sleep(4);
    }
    probe_log("exit");
    return 0;
}
