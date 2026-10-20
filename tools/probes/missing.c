#include <windows.h>

__declspec(dllimport) int carafe_missing(void);

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    return carafe_missing();
}
