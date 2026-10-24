#include "probe.h"

#define PROBE_KEY "Software\\CarafeProbe"
#define LINES 16

static char lines[LINES][160];
static int line_count;

static void note_line(const char *format, ...)
{
    va_list args;

    if (line_count >= LINES)
        return;
    va_start(args, format);
    vsnprintf(lines[line_count], sizeof(lines[line_count]), format, args);
    va_end(args);
    probe_log("%s", lines[line_count]);
    line_count++;
}

static void read_value(HKEY root, const char *root_name, const char *name)
{
    HKEY key;
    DWORD type = 0, size = 0;
    BYTE data[512];

    if (RegOpenKeyExA(root, PROBE_KEY, 0, KEY_READ, &key) != ERROR_SUCCESS)
    {
        note_line("missing %s\\%s", root_name, PROBE_KEY);
        return;
    }
    size = sizeof(data) - 2;
    if (RegQueryValueExA(key, name, NULL, &type, data, &size) != ERROR_SUCCESS)
        note_line("missing %s\\%s\\%s", root_name, PROBE_KEY, name);
    else if (type == REG_DWORD && size == 4)
        note_line("value %s\\%s\\%s = dword %lu", root_name, PROBE_KEY, name, (unsigned long)*(DWORD *)data);
    else if (type == REG_SZ || type == REG_EXPAND_SZ)
    {
        data[size] = 0;
        note_line("value %s\\%s\\%s = string %s", root_name, PROBE_KEY, name, (const char *)data);
    }
    else
        note_line("value %s\\%s\\%s = type %lu, %lu bytes", root_name, PROBE_KEY, name, (unsigned long)type,
                  (unsigned long)size);
    RegCloseKey(key);
}

static void bump_counter(void)
{
    HKEY key;
    DWORD type = 0, size = sizeof(DWORD), counter = 0;

    if (RegCreateKeyExA(HKEY_CURRENT_USER, PROBE_KEY, 0, NULL, 0, KEY_READ | KEY_WRITE, NULL, &key, NULL)
        != ERROR_SUCCESS)
    {
        note_line("cannot open HKCU\\%s for writing", PROBE_KEY);
        return;
    }
    if (RegQueryValueExA(key, "Starts", NULL, &type, (BYTE *)&counter, &size) != ERROR_SUCCESS || type != REG_DWORD)
        counter = 0;
    counter++;
    if (RegSetValueExA(key, "Starts", 0, REG_DWORD, (const BYTE *)&counter, sizeof(counter)) == ERROR_SUCCESS)
        note_line("wrote HKCU\\%s\\Starts = dword %lu", PROBE_KEY, (unsigned long)counter);
    RegFlushKey(key);
    RegCloseKey(key);
}

static LRESULT CALLBACK registry_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_PAINT)
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        RECT rect = {0, 0, PROBE_WIDTH, PROBE_HEIGHT};
        HBRUSH brush = CreateSolidBrush(RGB(32, 40, 64));
        HFONT font = CreateFontA(30, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE, DEFAULT_CHARSET, 0, 0, 0, 0, "Arial");
        HGDIOBJ old = SelectObject(dc, font);
        int i;

        FillRect(dc, &rect, brush);
        DeleteObject(brush);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, RGB(240, 240, 240));
        TextOutA(dc, 40, 30, "Carafe registry probe", 21);
        for (i = 0; i < line_count; i++)
            TextOutA(dc, 40, 90 + i * 38, lines[i], (int)strlen(lines[i]));
        SelectObject(dc, old);
        DeleteObject(font);
        EndPaint(window, &paint);
        return 0;
    }
    return probe_default_proc(window, message, wparam, lparam);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    static const char *const names[] = {"Width", "Height", "Language", "Old", "Starts", "Edited", "Added"};
    HWND window;
    unsigned int i;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("registry");
    for (i = 0; i < sizeof(names) / sizeof(*names); i++)
        read_value(HKEY_CURRENT_USER, "HKCU", names[i]);
    read_value(HKEY_LOCAL_MACHINE, "HKLM", "InstallPath");
    bump_counter();
    probe_log("registry probe done");
    window = probe_window("Carafe registry probe", registry_proc);
    while (probe_pump())
    {
        InvalidateRect(window, NULL, FALSE);
        Sleep(50);
    }
    probe_log("exit");
    return 0;
}
