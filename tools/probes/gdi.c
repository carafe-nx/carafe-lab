#include "probe.h"

static unsigned int frames;

static LRESULT CALLBACK gdi_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_PAINT)
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        int i;

        for (i = 0; i < 4; i++)
        {
            const struct probe_quadrant *q = &probe_quadrants[i];
            RECT rect = {q->left, q->top, q->right, q->bottom};
            HBRUSH brush = CreateSolidBrush(RGB((int)(q->r * 255), (int)(q->g * 255), (int)(q->b * 255)));
            FillRect(dc, &rect, brush);
            DeleteObject(brush);
        }
        EndPaint(window, &paint);
        probe_frame_done(++frames);
        return 0;
    }
    return probe_default_proc(window, message, wparam, lparam);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("gdi");
    window = probe_window("Carafe GDI probe", gdi_proc);
    while (probe_pump())
    {
        InvalidateRect(window, NULL, FALSE);
        Sleep(16);
    }
    probe_log("exit");
    return 0;
}
