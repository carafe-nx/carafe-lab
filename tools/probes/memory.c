#include "probe.h"

#define CHUNK_BYTES ((SIZE_T)16 * 1024 * 1024)
#define MAX_CHUNKS 176

static char progress[128] = "Reading memory limits";

static LRESULT CALLBACK memory_window(HWND window, UINT message, WPARAM word, LPARAM data)
{
    if (message == WM_TIMER)
    {
        InvalidateRect(window, NULL, FALSE);
        return 0;
    }
    if (message == WM_PAINT)
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        RECT rect;
        char caption[192];
        GetClientRect(window, &rect);
        FillRect(dc, &rect, (HBRUSH)GetStockObject(WHITE_BRUSH));
        SetTextColor(dc, RGB(0, 0, 0));
        snprintf(caption, sizeof(caption), "%s | tick %lu", progress, GetTickCount());
        DrawTextA(dc, caption, -1, &rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        EndPaint(window, &paint);
        return 0;
    }
    return probe_default_proc(window, message, word, data);
}

static MEMORYSTATUSEX memory_status(const char *stage)
{
    MEMORYSTATUSEX status = {0};
    status.dwLength = sizeof(status);
    if (!GlobalMemoryStatusEx(&status))
    {
        probe_log("memory status failed stage=%s error=%lu", stage, GetLastError());
        ExitProcess(2);
    }
    probe_log("memory stage=%s physical_total=%llu physical_available=%llu commit_total=%llu commit_available=%llu virtual_total=%llu virtual_available=%llu",
              stage, status.ullTotalPhys, status.ullAvailPhys, status.ullTotalPageFile,
              status.ullAvailPageFile, status.ullTotalVirtual, status.ullAvailVirtual);
    return status;
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    void *blocks[MAX_CHUNKS] = {0};
    unsigned count = 0;
    BOOL valid = TRUE;
    BOOL closing = FALSE;
    unsigned maximum = command_line && strstr(command_line, "stress") ? MAX_CHUNKS : 16;
    SYSTEM_INFO system;
    HWND window;
    (void)instance;
    (void)previous;
    (void)show;
    probe_open_log("memory");
    window = probe_window("Carafe memory probe", memory_window);
    SetTimer(window, 1, 100, NULL);
    UpdateWindow(window);
    GetSystemInfo(&system);
    memory_status("initial");
    if (sizeof(SIZE_T) == 8)
    {
        SIZE_T bytes = (SIZE_T)(6ull * 1024 * 1024 * 1024);
        void *reservation = VirtualAlloc(NULL, bytes, MEM_RESERVE, PAGE_NOACCESS);
        probe_log("virtual-only reservation bytes=%llu address=%p error=%lu",
                  (unsigned long long)bytes, reservation, reservation ? 0ul : GetLastError());
        memory_status("reserved-only");
        if (reservation)
            VirtualFree(reservation, 0, MEM_RELEASE);
    }
    for (; count < maximum; ++count)
    {
        MEMORYSTATUSEX status = memory_status("before-commit");
        volatile unsigned char *pages;
        SIZE_T offset;
        if (!probe_pump())
        {
            closing = TRUE;
            break;
        }
        if (!status.ullTotalPhys || status.ullAvailPhys < 256ull * 1024 * 1024)
        {
            probe_log("stopped with physical headroom chunks=%u", count);
            break;
        }
        blocks[count] = VirtualAlloc(NULL, CHUNK_BYTES, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (!blocks[count])
        {
            probe_log("commit failed chunks=%u error=%lu", count, GetLastError());
            break;
        }
        pages = blocks[count];
        for (offset = 0; offset < CHUNK_BYTES; offset += system.dwPageSize)
            pages[offset] = (unsigned char)(count + 1);
        probe_log("committed-and-touched bytes=%llu", (unsigned long long)(count + 1) * CHUNK_BYTES);
        snprintf(progress, sizeof(progress), "Committed and touched: %u MiB", (count + 1) * 16);
        InvalidateRect(window, NULL, FALSE);
        UpdateWindow(window);
        Sleep(100);
    }
    memory_status("held");
    Sleep(5000);
    for (unsigned index = 0; index < count; ++index)
    {
        volatile unsigned char *pages = blocks[index];
        for (SIZE_T offset = 0; offset < CHUNK_BYTES; offset += system.dwPageSize)
        {
            if (pages[offset] != (unsigned char)(index + 1))
            {
                probe_log("data mismatch chunk=%u offset=%llu", index, (unsigned long long)offset);
                valid = FALSE;
                break;
            }
        }
        if (!VirtualFree(blocks[index], 0, MEM_RELEASE))
        {
            probe_log("release failed chunk=%u error=%lu", index, GetLastError());
            valid = FALSE;
        }
    }
    memory_status("released");
    snprintf(progress, sizeof(progress), "%s; released %u MiB; probe-memory.log", valid ? "Verified" : "FAILED", count * 16);
    InvalidateRect(window, NULL, FALSE);
    UpdateWindow(window);
    probe_log("completed valid=%u chunks=%u bytes=%llu", valid, count, (unsigned long long)count * CHUNK_BYTES);
    {
        DWORD start = GetTickCount();
        while (!closing && GetTickCount() - start < 30000 && probe_pump())
        {
            Sleep(10);
        }
    }
    if (probe_log_file)
        fclose(probe_log_file);
    return valid ? 0 : 2;
}
