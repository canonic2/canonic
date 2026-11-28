#define UNICODE
#define _UNICODE
#include <windows.h>
#include "faults.hpp"
namespace {
AcmeDesktopFaults faults;
HWND corrected, armed, status;
LRESULT CALLBACK windowProc(HWND window, UINT message, WPARAM param, LPARAM data) {
    if (message == WM_CREATE) {
        HINSTANCE instance = reinterpret_cast<LPCREATESTRUCT>(data)->hInstance;
        CreateWindowW(L"STATIC", L"Acme native Windows diagnostics", WS_VISIBLE|WS_CHILD, 20, 15, 430, 25, window, nullptr, instance, nullptr);
        corrected = CreateWindowW(L"BUTTON", L"Corrected mode", WS_VISIBLE|WS_CHILD|BS_AUTOCHECKBOX, 20, 45, 430, 25, window, reinterpret_cast<HMENU>(1), instance, nullptr);
        armed = CreateWindowW(L"BUTTON", L"Arm app crash / 6-second hang", WS_VISIBLE|WS_CHILD|BS_AUTOCHECKBOX, 20, 75, 430, 25, window, reinterpret_cast<HMENU>(2), instance, nullptr);
        const wchar_t* titles[] = {L"Caught C++ exception", L"Bounded CPU work", L"Retain one MiB", L"Reset", L"C++ crash", L"UI-thread hang (6 seconds)", L"Background crash"};
        const int ids[] = {10,11,12,13,20,21,22};
        for (int n=0; n<7; ++n) CreateWindowW(L"BUTTON", titles[n], WS_VISIBLE|WS_CHILD|BS_PUSHBUTTON, 20, 110+n*38, 430, 32, window, reinterpret_cast<HMENU>(static_cast<INT_PTR>(ids[n])), instance, nullptr);
        status = CreateWindowW(L"STATIC", L"Acme ready", WS_VISIBLE|WS_CHILD, 20, 385, 430, 55, window, nullptr, instance, nullptr);
        return 0;
    }
    if (message == WM_COMMAND && LOWORD(param) >= 10) {
        faults.corrected = SendMessageW(corrected, BM_GETCHECK, 0, 0) == BST_CHECKED;
        faults.armed = SendMessageW(armed, BM_GETCHECK, 0, 0) == BST_CHECKED;
        std::string result;
        try { result = faults.run(LOWORD(param)); } catch (const std::exception& error) { result = error.what(); }
        SendMessageW(armed, BM_SETCHECK, faults.armed ? BST_CHECKED : BST_UNCHECKED, 0);
        SetWindowTextA(status, result.c_str()); return 0;
    }
    if (message == WM_DESTROY) { faults.run(13); PostQuitMessage(0); return 0; }
    return DefWindowProcW(window, message, param, data);
}
}
int WINAPI WinMain(HINSTANCE instance, HINSTANCE, LPSTR, int show) {
    WNDCLASSW type{}; type.lpfnWndProc = windowProc; type.hInstance = instance; type.lpszClassName = L"AcmeDiagnostics"; type.hCursor = LoadCursor(nullptr, IDC_ARROW); type.hbrBackground = reinterpret_cast<HBRUSH>(COLOR_WINDOW+1);
    if (!RegisterClassW(&type)) return 1;
    HWND window = CreateWindowW(type.lpszClassName, L"Acme Diagnostics", WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 490, 500, nullptr, nullptr, instance, nullptr);
    if (!window) return 1;
    ShowWindow(window, show); MSG message{}; int result;
    while ((result = GetMessageW(&message, nullptr, 0, 0)) > 0) { TranslateMessage(&message); DispatchMessageW(&message); }
    return result == -1 ? 1 : static_cast<int>(message.wParam);
}
