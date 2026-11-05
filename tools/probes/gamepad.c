#include "probe.h"

#define COBJMACROS
#define DIRECTINPUT_VERSION 0x0800
#include <dinput.h>
#include <xinput.h>
#include <setupapi.h>
#include <hidsdi.h>
#include <cfgmgr32.h>
#include <roapi.h>
#include <winstring.h>
#include <windows.gaming.input.h>

#define SHOWN_LINES 24
#define DINPUT_BUTTONS 32
#define COUNT_INTERVAL_MS 1000

typedef DWORD (WINAPI *get_state_fn)(DWORD, XINPUT_STATE *);
typedef HRESULT (WINAPI *ro_initialize_fn)(RO_INIT_TYPE);
typedef HRESULT (WINAPI *ro_factory_fn)(HSTRING, REFIID, void **);
typedef HRESULT (WINAPI *create_string_fn)(const WCHAR *, UINT32, HSTRING *);
typedef HRESULT (WINAPI *delete_string_fn)(HSTRING);

typedef __x_ABI_CWindows_CGaming_CInput_CIGamepadStatics gamepad_statics;
typedef __x_ABI_CWindows_CGaming_CInput_CIRawGameControllerStatics raw_statics;
typedef __x_ABI_CWindows_CGaming_CInput_CIGamepad gamepad;
typedef __x_ABI_CWindows_CGaming_CInput_CGamepadReading gamepad_reading;
typedef __FIVectorView_1_Windows__CGaming__CInput__CGamepad gamepad_view;
typedef __FIVectorView_1_Windows__CGaming__CInput__CRawGameController raw_view;

static const GUID iid_gamepad_statics = {0x8bbce529, 0xd49c, 0x39e9, {0x95, 0x60, 0xe4, 0x7d, 0xde, 0x96, 0xb7, 0xc8}};
static const GUID iid_raw_statics = {0xeb8d0792, 0xe95a, 0x4b19, {0xaf, 0xc7, 0x0a, 0x59, 0xf8, 0xbf, 0x75, 0x9e}};
static const GUID guid_winexinput = {0x6c53d5fd, 0x6480, 0x440f, {0xb6, 0x18, 0x47, 0x67, 0x50, 0xc5, 0xe1, 0xa6}};

static char shown[SHOWN_LINES][112];
static int shown_count;

static void remember(const char *format, ...)
{
    char line[112];
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

static void draw_lines(HDC dc)
{
    RECT all = {0, 0, PROBE_WIDTH, PROBE_HEIGHT};
    int i;

    FillRect(dc, &all, (HBRUSH)GetStockObject(BLACK_BRUSH));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, RGB(230, 230, 230));
    TextOutA(dc, 16, 8, "Carafe gamepad probe", 20);
    for (i = 0; i < shown_count; i++)
        TextOutA(dc, 16, 36 + i * 26, shown[i], (int)strlen(shown[i]));
}

static LRESULT CALLBACK gamepad_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_PAINT)
    {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);

        draw_lines(dc);
        EndPaint(window, &paint);
        return 0;
    }
    return probe_default_proc(window, message, wparam, lparam);
}

static void list_interfaces(const GUID *guid, const char *kind)
{
    SP_DEVICE_INTERFACE_DATA iface = {.cbSize = sizeof(iface)};
    char buffer[sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_A) + MAX_PATH];
    SP_DEVICE_INTERFACE_DETAIL_DATA_A *detail = (void *)buffer;
    HDEVINFO set = SetupDiGetClassDevsA(guid, NULL, NULL, DIGCF_DEVICEINTERFACE | DIGCF_PRESENT);
    DWORD i, count = 0;

    if (set == INVALID_HANDLE_VALUE)
    {
        probe_log("%s list failed %lu", kind, GetLastError());
        return;
    }
    for (i = 0; SetupDiEnumDeviceInterfaces(set, NULL, guid, i, &iface); i++)
    {
        HIDD_ATTRIBUTES attrs = {.Size = sizeof(attrs)};
        PHIDP_PREPARSED_DATA preparsed = NULL;
        HIDP_CAPS caps = {0};
        WCHAR product[128] = {0};
        HANDLE file;

        detail->cbSize = sizeof(*detail);
        if (!SetupDiGetDeviceInterfaceDetailA(set, &iface, detail, sizeof(buffer), NULL, NULL))
            continue;
        count++;
        file = CreateFileA(detail->DevicePath, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL,
                           OPEN_EXISTING, 0, NULL);
        if (file == INVALID_HANDLE_VALUE)
        {
            remember("%s %s open failed %lu", kind, detail->DevicePath, GetLastError());
            continue;
        }
        HidD_GetAttributes(file, &attrs);
        HidD_GetProductString(file, product, sizeof(product));
        if (HidD_GetPreparsedData(file, &preparsed))
        {
            HidP_GetCaps(preparsed, &caps);
            HidD_FreePreparsedData(preparsed);
        }
        remember("%s vid=%04x pid=%04x usage=%02x:%02x input=%u buttons=%u values=%u name=%ls", kind,
                 attrs.VendorID, attrs.ProductID, caps.UsagePage, caps.Usage, caps.InputReportByteLength,
                 caps.NumberInputButtonCaps, caps.NumberInputValueCaps, product);
        probe_log("%s path %s", kind, detail->DevicePath);
        CloseHandle(file);
    }
    SetupDiDestroyDeviceInfoList(set);
    remember("%s devices %lu", kind, count);
}

static void list_keys(const char *path)
{
    char name[256];
    DWORD i, size;
    HKEY key;
    LONG result = RegOpenKeyExA(HKEY_LOCAL_MACHINE, path, 0, KEY_READ, &key);

    if (result)
    {
        probe_log("registry %s missing %ld", path, result);
        return;
    }
    for (i = 0; size = sizeof(name), !RegEnumKeyExA(key, i, name, &size, NULL, NULL, NULL, NULL); i++)
        probe_log("registry %s\\%s", path, name);
    probe_log("registry %s has %lu keys", path, i);
    RegCloseKey(key);
}

static void show_value(const char *path, const char *name)
{
    char data[256] = {0};
    DWORD size = sizeof(data) - 1, type = 0;
    LONG result = RegGetValueA(HKEY_LOCAL_MACHINE, path, name, RRF_RT_ANY, &type, data, &size);

    if (result)
        probe_log("value %s\\%s missing %ld", path, name, result);
    else if (type == REG_DWORD)
        probe_log("value %s\\%s dword %lu", path, name, *(DWORD *)data);
    else
        probe_log("value %s\\%s type %lu %s", path, name, type, data);
}

static void list_cfgmgr(const GUID *guid, const char *kind)
{
    WCHAR list[2048];
    ULONG size = 0;
    CONFIGRET result = CM_Get_Device_Interface_List_SizeW(&size, (GUID *)guid, NULL, CM_GET_DEVICE_INTERFACE_LIST_PRESENT);

    probe_log("cfgmgr %s size result %lu size %lu", kind, result, size);
    result = CM_Get_Device_Interface_ListW((GUID *)guid, NULL, list, sizeof(list) / sizeof(list[0]),
                                           CM_GET_DEVICE_INTERFACE_LIST_PRESENT);
    probe_log("cfgmgr %s list result %lu first %ls", kind, result, result ? L"" : list);
}

static void show_registry(void)
{
    static const char *const devices[] = {"IG_00", "XI_00"};
    static const char *const classes[] = {"{4d1e55b2-f16f-11cf-88cb-001111000030}", "{6c53d5fd-6480-440f-b618-476750c5e1a6}"};
    char path[512];
    int i;

    for (i = 0; i < 2; i++)
    {
        snprintf(path, sizeof(path), "System\\CurrentControlSet\\Enum\\HID\\VID_045E&PID_028E&%s\\0000", devices[i]);
        show_value(path, "ClassGUID");
        snprintf(path, sizeof(path), "System\\CurrentControlSet\\Control\\DeviceClasses\\%s\\##?#HID#VID_045E&PID_028E&%s#0000#%s",
                 classes[i], devices[i], classes[i]);
        show_value(path, "DeviceInstance");
        strcat(path, "\\#");
        show_value(path, "SymbolicLink");
        strcat(path, "\\Control");
        show_value(path, "Linked");
    }
}

static void list_rawinput(void)
{
    RAWINPUTDEVICELIST list[32];
    UINT count = sizeof(list) / sizeof(list[0]), i, hid = 0;
    UINT got = GetRawInputDeviceList(list, &count, sizeof(list[0]));

    if (got == (UINT)-1)
    {
        remember("rawinput list failed %lu", GetLastError());
        return;
    }
    for (i = 0; i < got; i++)
    {
        RID_DEVICE_INFO info = {.cbSize = sizeof(info)};
        UINT size = sizeof(info);

        if (list[i].dwType != RIM_TYPEHID)
            continue;
        hid++;
        if (GetRawInputDeviceInfoA(list[i].hDevice, RIDI_DEVICEINFO, &info, &size) != (UINT)-1)
            remember("rawinput hid vid=%04lx pid=%04lx usage=%02x:%02x", info.hid.dwVendorId, info.hid.dwProductId,
                     info.hid.usUsagePage, info.hid.usUsage);
    }
    remember("rawinput devices %u hid %u", got, hid);
}

static IDirectInputDevice8A *joystick;
static DIJOYSTATE2 joystick_last;
static BOOL joystick_have_last;

static BOOL CALLBACK enum_joystick(const DIDEVICEINSTANCEA *instance, void *context)
{
    IDirectInput8A *dinput = context;
    DIDEVCAPS caps = {.dwSize = sizeof(caps)};

    remember("dinput8 device %s type=%02lx product=%08lx", instance->tszProductName,
             (unsigned long)GET_DIDEVICE_TYPE(instance->dwDevType), instance->guidProduct.Data1);
    if (joystick)
        return DIENUM_CONTINUE;
    if (FAILED(IDirectInput8_CreateDevice(dinput, &instance->guidInstance, &joystick, NULL)))
    {
        remember("dinput8 create failed");
        return DIENUM_CONTINUE;
    }
    IDirectInputDevice8_SetDataFormat(joystick, &c_dfDIJoystick2);
    IDirectInputDevice8_SetCooperativeLevel(joystick, NULL, DISCL_BACKGROUND | DISCL_NONEXCLUSIVE);
    IDirectInputDevice8_GetCapabilities(joystick, &caps);
    remember("dinput8 axes=%lu buttons=%lu povs=%lu acquire=0x%08lx", caps.dwAxes, caps.dwButtons, caps.dwPOVs,
             (unsigned long)IDirectInputDevice8_Acquire(joystick));
    return DIENUM_CONTINUE;
}

static void open_dinput8(void)
{
    IDirectInput8A *dinput = NULL;
    HRESULT hr = DirectInput8Create(GetModuleHandleA(NULL), DIRECTINPUT_VERSION, &IID_IDirectInput8A,
                                    (void **)&dinput, NULL);

    if (FAILED(hr))
    {
        remember("dinput8 missing 0x%08lx", (unsigned long)hr);
        return;
    }
    IDirectInput8_EnumDevices(dinput, DI8DEVCLASS_GAMECTRL, enum_joystick, dinput, DIEDFL_ATTACHEDONLY);
    if (!joystick)
        remember("dinput8 no joystick");
}

static void poll_dinput8(void)
{
    DIJOYSTATE2 state;
    DWORD buttons = 0;
    int i;

    if (!joystick)
        return;
    IDirectInputDevice8_Poll(joystick);
    if (FAILED(IDirectInputDevice8_GetDeviceState(joystick, sizeof(state), &state)))
    {
        IDirectInputDevice8_Acquire(joystick);
        return;
    }
    if (joystick_have_last && !memcmp(&state, &joystick_last, sizeof(state)))
        return;
    for (i = 0; i < DINPUT_BUTTONS; i++)
        if (state.rgbButtons[i] & 0x80)
            buttons |= 1u << i;
    remember("dinput8 buttons=0x%08lx pov=%ld x=%ld y=%ld z=%ld rx=%ld ry=%ld rz=%ld", buttons, (long)state.rgdwPOV[0],
             state.lX, state.lY, state.lZ, state.lRx, state.lRy, state.lRz);
    joystick_last = state;
    joystick_have_last = TRUE;
}

static gamepad_statics *wgi_gamepads;
static raw_statics *wgi_raw;
static gamepad_reading wgi_last;
static BOOL wgi_have_last;
static UINT32 wgi_count = (UINT32)-1, wgi_raw_count = (UINT32)-1;

static void *activation_factory(HMODULE combase, const WCHAR *name, const GUID *iid)
{
    ro_factory_fn factory = (ro_factory_fn)(void *)GetProcAddress(combase, "RoGetActivationFactory");
    create_string_fn create = (create_string_fn)(void *)GetProcAddress(combase, "WindowsCreateString");
    delete_string_fn destroy = (delete_string_fn)(void *)GetProcAddress(combase, "WindowsDeleteString");
    HSTRING string;
    void *out = NULL;
    HRESULT hr;

    if (!factory || !create || !destroy || FAILED(create(name, (UINT32)wcslen(name), &string)))
        return NULL;
    hr = factory(string, iid, &out);
    destroy(string);
    if (FAILED(hr))
        remember("wgi factory %ls failed 0x%08lx", name, (unsigned long)hr);
    return SUCCEEDED(hr) ? out : NULL;
}

static void open_wgi(void)
{
    HMODULE combase = LoadLibraryA("combase.dll");
    ro_initialize_fn initialize = combase ? (ro_initialize_fn)(void *)GetProcAddress(combase, "RoInitialize") : NULL;

    if (!initialize)
    {
        remember("wgi missing");
        return;
    }
    initialize(RO_INIT_MULTITHREADED);
    wgi_gamepads = activation_factory(combase, L"Windows.Gaming.Input.Gamepad", &iid_gamepad_statics);
    wgi_raw = activation_factory(combase, L"Windows.Gaming.Input.RawGameController", &iid_raw_statics);
}

static void count_wgi(void)
{
    gamepad_view *pads = NULL;
    raw_view *raws = NULL;
    UINT32 count = 0;

    if (wgi_gamepads && SUCCEEDED(__x_ABI_CWindows_CGaming_CInput_CIGamepadStatics_get_Gamepads(wgi_gamepads, &pads)))
    {
        __FIVectorView_1_Windows__CGaming__CInput__CGamepad_get_Size(pads, &count);
        __FIVectorView_1_Windows__CGaming__CInput__CGamepad_Release(pads);
        if (count != wgi_count)
            remember("wgi gamepads %u", count);
        wgi_count = count;
    }
    count = 0;
    if (wgi_raw &&
        SUCCEEDED(__x_ABI_CWindows_CGaming_CInput_CIRawGameControllerStatics_get_RawGameControllers(wgi_raw, &raws)))
    {
        __FIVectorView_1_Windows__CGaming__CInput__CRawGameController_get_Size(raws, &count);
        __FIVectorView_1_Windows__CGaming__CInput__CRawGameController_Release(raws);
        if (count != wgi_raw_count)
            remember("wgi raw controllers %u", count);
        wgi_raw_count = count;
    }
}

static void poll_wgi(void)
{
    gamepad_view *pads = NULL;
    gamepad *pad = NULL;
    gamepad_reading reading;
    UINT32 count = 0;

    if (!wgi_gamepads || FAILED(__x_ABI_CWindows_CGaming_CInput_CIGamepadStatics_get_Gamepads(wgi_gamepads, &pads)))
        return;
    __FIVectorView_1_Windows__CGaming__CInput__CGamepad_get_Size(pads, &count);
    if (count && SUCCEEDED(__FIVectorView_1_Windows__CGaming__CInput__CGamepad_GetAt(pads, 0, &pad)))
    {
        if (SUCCEEDED(__x_ABI_CWindows_CGaming_CInput_CIGamepad_GetCurrentReading(pad, &reading)))
        {
            reading.Timestamp = 0;
            if (!wgi_have_last || memcmp(&reading, &wgi_last, sizeof(reading)))
            {
                remember("wgi buttons=0x%04x lt=%.2f rt=%.2f lx=%.2f ly=%.2f rx=%.2f ry=%.2f", (unsigned)reading.Buttons,
                         reading.LeftTrigger, reading.RightTrigger, reading.LeftThumbstickX, reading.LeftThumbstickY,
                         reading.RightThumbstickX, reading.RightThumbstickY);
                wgi_last = reading;
                wgi_have_last = TRUE;
            }
        }
        __x_ABI_CWindows_CGaming_CInput_CIGamepad_Release(pad);
    }
    __FIVectorView_1_Windows__CGaming__CInput__CGamepad_Release(pads);
}

static get_state_fn load_xinput(void)
{
    HMODULE module = LoadLibraryA("xinput1_4.dll");
    get_state_fn get_state = module ? (get_state_fn)(void *)GetProcAddress(module, "XInputGetState") : NULL;

    remember(get_state ? "xinput ready" : "xinput missing");
    return get_state;
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    HWND window;
    GUID hid_guid;
    get_state_fn get_state;
    XINPUT_STATE state, last;
    BOOL have_last = FALSE;
    DWORD last_count = 0, last_paint = 0;

    (void)instance;
    (void)previous;
    (void)command_line;
    (void)show;
    probe_open_log("gamepad");
    window = probe_window("Carafe gamepad probe", gamepad_proc);
    list_keys("System\\CurrentControlSet\\Enum\\HID");
    list_keys("System\\CurrentControlSet\\Control\\DeviceClasses\\{4d1e55b2-f16f-11cf-88cb-001111000030}");
    list_keys("System\\CurrentControlSet\\Control\\DeviceClasses\\{6c53d5fd-6480-440f-b618-476750c5e1a6}");
    show_registry();
    HidD_GetHidGuid(&hid_guid);
    list_cfgmgr(&hid_guid, "hid");
    list_cfgmgr(&guid_winexinput, "xi");
    list_interfaces(&hid_guid, "hid");
    list_interfaces(&guid_winexinput, "xi");
    list_rawinput();
    open_dinput8();
    open_wgi();
    get_state = load_xinput();
    memset(&last, 0, sizeof(last));
    while (probe_pump())
    {
        if (get_state)
        {
            memset(&state, 0, sizeof(state));
            if (get_state(0, &state) == ERROR_SUCCESS &&
                (!have_last || memcmp(&state.Gamepad, &last.Gamepad, sizeof(state.Gamepad))))
            {
                remember("xinput buttons=0x%04x lt=%u rt=%u lx=%d ly=%d rx=%d ry=%d", state.Gamepad.wButtons,
                         state.Gamepad.bLeftTrigger, state.Gamepad.bRightTrigger, state.Gamepad.sThumbLX,
                         state.Gamepad.sThumbLY, state.Gamepad.sThumbRX, state.Gamepad.sThumbRY);
                last = state;
                have_last = TRUE;
            }
        }
        poll_dinput8();
        poll_wgi();
        if (GetTickCount() - last_count >= COUNT_INTERVAL_MS)
        {
            count_wgi();
            last_count = GetTickCount();
        }
        if (GetTickCount() - last_paint >= 40)
        {
            InvalidateRect(window, NULL, FALSE);
            last_paint = GetTickCount();
        }
        MsgWaitForMultipleObjects(0, NULL, FALSE, 4, QS_ALLINPUT);
    }
    probe_log("exit");
    return 0;
}
