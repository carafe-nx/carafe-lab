#include <winsock2.h>
#include <ws2tcpip.h>

#include "probe.h"

#define LINES 18
#define PROBE_PORT 9989
#define CLOSED_PORT 9990
#define READS 4
#define BUFFER_SIZE 4096

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

static void read_all(const char *where, SOCKET sock)
{
    char buffer[BUFFER_SIZE];
    struct sockaddr_in from;
    u_long pending = 0;
    int i;

    for (i = 0; i < READS; i++)
    {
        int from_len = sizeof(from);
        int got;

        memset(&from, 0, sizeof(from));
        got = recvfrom(sock, buffer, sizeof(buffer), 0, (struct sockaddr *)&from, &from_len);
        if (got == SOCKET_ERROR)
            note_line("%s read %d: error %d", where, i, WSAGetLastError());
        else
            note_line("%s read %d: %d bytes from %s:%u", where, i, got, inet_ntoa(from.sin_addr),
                      (unsigned)ntohs(from.sin_port));
    }
    if (ioctlsocket(sock, FIONREAD, &pending))
        note_line("%s FIONREAD error %d", where, WSAGetLastError());
    else
        note_line("%s FIONREAD %lu", where, (unsigned long)pending);
}

static SOCKET open_udp(const char *where, unsigned short port)
{
    struct sockaddr_in address;
    u_long nonblocking = 1;
    BOOL on = TRUE;
    SOCKET sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);

    if (sock == INVALID_SOCKET)
    {
        note_line("%s socket error %d", where, WSAGetLastError());
        return sock;
    }
    if (setsockopt(sock, SOL_SOCKET, SO_REUSEADDR, (const char *)&on, sizeof(on)))
        note_line("%s SO_REUSEADDR error %d", where, WSAGetLastError());
    if (setsockopt(sock, SOL_SOCKET, SO_BROADCAST, (const char *)&on, sizeof(on)))
        note_line("%s SO_BROADCAST error %d", where, WSAGetLastError());
    if (ioctlsocket(sock, FIONBIO, &nonblocking))
        note_line("%s FIONBIO error %d", where, WSAGetLastError());
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_ANY);
    address.sin_port = htons(port);
    if (bind(sock, (struct sockaddr *)&address, sizeof(address)))
        note_line("%s bind %u error %d", where, (unsigned)port, WSAGetLastError());
    return sock;
}

static void check_sockets(void)
{
    struct sockaddr_in closed;
    WSADATA data;
    SOCKET sock;
    int sent;

    if (WSAStartup(MAKEWORD(2, 2), &data))
    {
        note_line("WSAStartup failed");
        return;
    }
    sock = open_udp("empty", PROBE_PORT);
    if (sock == INVALID_SOCKET)
        return;
    read_all("empty", sock);

    memset(&closed, 0, sizeof(closed));
    closed.sin_family = AF_INET;
    closed.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    closed.sin_port = htons(CLOSED_PORT);
    sent = sendto(sock, "carafe", 6, 0, (struct sockaddr *)&closed, sizeof(closed));
    if (sent == SOCKET_ERROR)
        note_line("refused sendto error %d", WSAGetLastError());
    else
        note_line("refused sendto %d bytes", sent);
    Sleep(100);
    read_all("refused", sock);
    closesocket(sock);
    WSACleanup();
}

static LRESULT CALLBACK sockets_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_PAINT)
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        RECT rect = {0, 0, PROBE_WIDTH, PROBE_HEIGHT};
        HBRUSH brush = CreateSolidBrush(RGB(32, 40, 48));
        HFONT font = CreateFontA(28, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE, DEFAULT_CHARSET, 0, 0, 0, 0, "Arial");
        HGDIOBJ old = SelectObject(dc, font);
        int i;

        FillRect(dc, &rect, brush);
        DeleteObject(brush);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, RGB(240, 240, 240));
        TextOutA(dc, 40, 20, "Carafe sockets probe", 20);
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
    HWND window;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("sockets");
    check_sockets();
    probe_log("sockets probe done");
    window = probe_window("Carafe sockets probe", sockets_proc);
    while (probe_pump())
    {
        InvalidateRect(window, NULL, FALSE);
        Sleep(50);
    }
    probe_log("exit");
    return 0;
}
