#include "probe.h"

#define LINES 18

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

static void note_result(const char *where, const char *what, BOOL ok)
{
    if (ok)
        note_line("%s %s ok", where, what);
    else
        note_line("%s %s failed %lu", where, what, (unsigned long)GetLastError());
}

static BOOL lock_at(HANDLE file, ULONGLONG offset)
{
    OVERLAPPED overlapped = {0};

    overlapped.Offset = (DWORD)offset;
    overlapped.OffsetHigh = (DWORD)(offset >> 32);
    return LockFileEx(file, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &overlapped);
}

static BOOL unlock_at(HANDLE file, ULONGLONG offset)
{
    OVERLAPPED overlapped = {0};

    overlapped.Offset = (DWORD)offset;
    overlapped.OffsetHigh = (DWORD)(offset >> 32);
    return UnlockFileEx(file, 0, 1, 0, &overlapped);
}

static BOOL write_at(HANDLE file, const void *data, DWORD size, ULONGLONG offset)
{
    OVERLAPPED overlapped = {0};
    DWORD written = 0;

    overlapped.Offset = (DWORD)offset;
    overlapped.OffsetHigh = (DWORD)(offset >> 32);
    return WriteFile(file, data, size, &written, &overlapped) && written == size;
}

static void check_folder(const char *where, const char *folder)
{
    static const char magic[8] = "FOZPROB";
    char path[MAX_PATH];
    char back[8] = {0};
    LARGE_INTEGER size;
    OVERLAPPED overlapped = {0};
    DWORD read = 0;
    HANDLE file;

    CreateDirectoryA(folder, NULL);
    snprintf(path, sizeof(path), "%s\\probe.foz", folder);
    DeleteFileA(path);
    file = CreateFileA(path, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, NULL,
                       OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
    {
        note_line("%s open failed %lu", where, (unsigned long)GetLastError());
        return;
    }
    note_result(where, "size", GetFileSizeEx(file, &size));
    note_result(where, "lock-high", lock_at(file, 1ULL << 62));
    note_result(where, "lock-low", lock_at(file, 0));
    note_result(where, "write-at-0", write_at(file, magic, sizeof(magic), 0));
    note_result(where, "write-at-end", write_at(file, magic, sizeof(magic), sizeof(magic)));
    note_result(where, "read-at-0", ReadFile(file, back, sizeof(back), &read, &overlapped) && read == sizeof(back)
                                        && !memcmp(back, magic, sizeof(magic)));
    note_result(where, "unlock-high", unlock_at(file, 1ULL << 62));
    note_result(where, "unlock-low", unlock_at(file, 0));
    CloseHandle(file);
}

static LRESULT CALLBACK files_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_PAINT)
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        RECT rect = {0, 0, PROBE_WIDTH, PROBE_HEIGHT};
        HBRUSH brush = CreateSolidBrush(RGB(32, 48, 40));
        HFONT font = CreateFontA(28, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE, DEFAULT_CHARSET, 0, 0, 0, 0, "Arial");
        HGDIOBJ old = SelectObject(dc, font);
        int i;

        FillRect(dc, &rect, brush);
        DeleteObject(brush);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, RGB(240, 240, 240));
        TextOutA(dc, 40, 20, "Carafe files probe", 18);
        for (i = 0; i < line_count; i++)
            TextOutA(dc, 40, 70 + i * 34, lines[i], (int)strlen(lines[i]));
        SelectObject(dc, old);
        DeleteObject(font);
        EndPaint(window, &paint);
        return 0;
    }
    return probe_default_proc(window, message, wparam, lparam);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    char local[MAX_PATH], folder[MAX_PATH];
    HWND window;
    DWORD length;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("files");
    length = GetEnvironmentVariableA("LOCALAPPDATA", local, sizeof(local));
    if (!length || length >= sizeof(local))
        snprintf(local, sizeof(local), "C:\\users\\steamuser\\AppData\\Local");
    snprintf(folder, sizeof(folder), "%s\\fex-emu", local);
    CreateDirectoryA(folder, NULL);
    snprintf(folder, sizeof(folder), "%s\\fex-emu\\probe", local);
    check_folder("sd", folder);
    snprintf(folder, sizeof(folder), "%s\\probe-files", local);
    check_folder("save", folder);
    probe_log("files probe done");
    window = probe_window("Carafe files probe", files_proc);
    while (probe_pump())
    {
        InvalidateRect(window, NULL, FALSE);
        Sleep(50);
    }
    probe_log("exit");
    return 0;
}
