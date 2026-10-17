#include "probe.h"

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

static void poll_cursor(void)
{
    POINT cursor;

    if (!GetCursorPos(&cursor))
        return;
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

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("input");
    window = probe_window("Carafe input probe", input_proc);
    get_state = load_xinput();
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
        if (GetTickCount() - last_move >= 100)
        {
            if (move_x || move_y)
            {
                remember("move %ld %ld", move_x, move_y);
                move_x = move_y = 0;
                InvalidateRect(window, NULL, FALSE);
            }
            last_move = GetTickCount();
        }
        Sleep(4);
    }
    probe_log("exit");
    return 0;
}
