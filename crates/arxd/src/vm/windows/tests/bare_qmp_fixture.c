// A sealed QMP peer built in the test's scratch directory. No VM is created.
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <windows.h>

#if defined(FIXTURE_BCRYPT)
#include <bcrypt.h>
#endif

#if defined(FIXTURE_QEMU_SHAPE)
#include <objbase.h>
#include <mmsystem.h>
#include <wincrypt.h>

// QEMU's system import set includes libraries outside this startup path.
#pragma comment(linker, "/include:__imp_CommandLineToArgvW")
#pragma comment(linker, "/include:__imp_PathIsRelativeA")
#pragma comment(linker, "/include:__imp_DnsFree")
#pragma comment(linker, "/include:__imp_GetAdaptersInfo")

typedef HRESULT (WINAPI *Capability)(int, void *, UINT32, UINT32 *);
#endif

static const char greeting[] = "{\"QMP\":{\"version\":{\"qemu\":{\"major\":11,\"minor\":1,\"micro\":3},\"package\":\"\"},\"capabilities\":[]}}\r\n";
static const char request[] = "{\"execute\":\"qmp_capabilities\",\"id\":1}\r\n";
static const char ack[] = "{\"return\":{},\"id\":1}\r\n";
static const char marker[] = "fixture stderr marker";

static void require(BOOL passed, UINT status) {
    if (!passed) ExitProcess(status);
}

static void emit(HANDLE pipe, const char *bytes, DWORD length) {
    DWORD written = 0;
    require(WriteFile(pipe, bytes, length, &written, NULL) && written == length, 10);
}

static void expect(HANDLE pipe, const char *bytes, DWORD length) {
    for (DWORD i = 0; i < length; ++i) {
        DWORD read = 0;
        char byte = 0;
        require(ReadFile(pipe, &byte, 1, &read, NULL) && read == 1 && byte == bytes[i], 11);
    }
}

#if defined(FIXTURE_USER32) || defined(FIXTURE_QEMU_SHAPE)
static void verify_desktop(void) {
    require(GetDesktopWindow() != NULL, 20);
    HWINSTA station = GetProcessWindowStation();
    USEROBJECTFLAGS flags = {0};
    DWORD needed = 0;
    require(station && GetUserObjectInformationW(station, UOI_FLAGS, &flags, sizeof(flags), &needed), 21);
    require(!(flags.dwFlags & WSF_VISIBLE), 22);
    wchar_t name[128];
    require(GetUserObjectInformationW(station, UOI_NAME, name, sizeof(name), &needed), 23);
    require(CompareStringOrdinal(name, 8, L"Service-", 8, TRUE) == CSTR_EQUAL, 24);

    HWINSTA interactive = OpenWindowStationW(L"WinSta0", FALSE, MAXIMUM_ALLOWED);
    if (!interactive) {
        require(GetLastError() == ERROR_ACCESS_DENIED, 25);
        return;
    }
    if (SetProcessWindowStation(interactive)) {
        DWORD rights[] = {
            DESKTOP_ENUMERATE | DESKTOP_READOBJECTS,
            DESKTOP_WRITEOBJECTS | DESKTOP_HOOKCONTROL | DESKTOP_CREATEWINDOW,
        };
        for (DWORD i = 0; i < ARRAYSIZE(rights); ++i) {
            HDESK desktop = OpenDesktopW(L"Default", 0, FALSE, rights[i]);
            require(!desktop && GetLastError() == ERROR_ACCESS_DENIED, 26);
        }
        require(SetProcessWindowStation(station), 27);
    } else {
        require(GetLastError() == ERROR_ACCESS_DENIED, 28);
    }
    require(CloseWindowStation(interactive), 29);
}
#endif

#if defined(FIXTURE_QEMU_SHAPE)
static void startup(void) {
    HCRYPTPROV provider = 0;
    BYTE random[32];
    require(CryptAcquireContextA(&provider, NULL, NULL, PROV_RSA_FULL,
                                 CRYPT_SILENT | CRYPT_VERIFYCONTEXT), 30);
    require(CryptGenRandom(provider, sizeof(random), random), 31);
    require(CryptReleaseContext(provider, 0), 32);

    WSADATA data;
    require(WSAStartup(MAKEWORD(2, 2), &data) == 0, 33);
    SOCKET sock = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    require(sock != INVALID_SOCKET, 34);
    struct sockaddr_in address = {0};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    require(bind(sock, (struct sockaddr *)&address, sizeof(address)) == 0, 35);
    closesocket(sock);
    WSACleanup();

    require(SUCCEEDED(CoInitialize(NULL)), 36);
    TIMECAPS caps;
    require(timeGetDevCaps(&caps, sizeof(caps)) == MMSYSERR_NOERROR, 37);
    require(timeBeginPeriod(1) == TIMERR_NOERROR, 38);
    timeEndPeriod(1);
    CoUninitialize();

    wchar_t profile[2];
    BOOL full_bundle = GetEnvironmentVariableW(L"ARX_QMP_FIXTURE_PROBE_BUNDLE", profile, ARRAYSIZE(profile)) > 0;
    HMODULE whpx = LoadLibraryExW(L"WinHvPlatform.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
    require(whpx || !full_bundle, 39);
    if (whpx) {
        Capability query = (Capability)GetProcAddress(whpx, "WHvGetCapability");
        BOOL present = FALSE;
        UINT32 written = 0;
        require(query != NULL, 40);
        HRESULT result = query(0, &present, sizeof(present), &written);
        require(!full_bundle || (SUCCEEDED(result) && present), 41);
    }
    if (full_bundle) {
        const wchar_t *dlls[] = {
            L"libglib-2.0-0.dll", L"libgio-2.0-0.dll", L"libgobject-2.0-0.dll",
            L"libgmodule-2.0-0.dll", L"libslirp-0.dll", L"libpixman-1-0.dll",
            L"libzstd.dll", L"libz-1.dll", L"libbz2-1.dll", L"libncursesw6.dll",
            L"libiconv-2.dll", L"libwinpthread-1.dll",
        };
        for (DWORD i = 0; i < ARRAYSIZE(dlls); ++i) {
            require(LoadLibraryExW(dlls[i], NULL, LOAD_LIBRARY_SEARCH_APPLICATION_DIR |
                                  LOAD_LIBRARY_SEARCH_SYSTEM32) != NULL, 42 + i);
        }
    }
    require(GetModuleHandleW(L"bcrypt.dll") == NULL, 54);
}
#endif

void fixture_main(void) {
#if defined(FIXTURE_BCRYPT)
    // Force a static import. The restricted child must fail before entry.
    UCHAR bytes[8];
    require(BCryptGenRandom(NULL, bytes, sizeof(bytes), BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0, 60);
#endif
    HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
    HANDLE output = GetStdHandle(STD_OUTPUT_HANDLE);
    HANDLE error = GetStdHandle(STD_ERROR_HANDLE);
    require(GetConsoleCP() == 0, 12);
    require(GetFileType(input) == FILE_TYPE_PIPE && GetFileType(output) == FILE_TYPE_PIPE &&
            GetFileType(error) == FILE_TYPE_PIPE, 13);
#if defined(FIXTURE_USER32) || defined(FIXTURE_QEMU_SHAPE)
    verify_desktop();
#endif
#if defined(FIXTURE_QEMU_SHAPE)
    startup();
#endif
    char mode[8];
    DWORD length = GetEnvironmentVariableA("ARX_QMP_STDIO_FIXTURE", mode, sizeof(mode));
    require(length && length < sizeof(mode), 14);
    emit(error, marker, sizeof(marker) - 1);
    if (mode[0] == 'g' || mode[0] == 'z') {
        if (mode[0] == 'z') emit(output, "", 0);
        emit(output, greeting, sizeof(greeting) - 1);
        expect(input, request, sizeof(request) - 1);
        emit(output, ack, sizeof(ack) - 1);
    } else if (mode[0] == 'c') {
        require(CloseHandle(output), 15);
    } else {
        require(mode[0] == 's', 16);
    }
    expect(input, "!", 1);
    ExitProcess(0);
}

#if defined(FIXTURE_QEMU_SHAPE)
int main(void) {
    fixture_main();
    return 1;
}
#endif
