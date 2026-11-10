//! Hand written Win32 bindings, all imported the regular way.
#![allow(non_snake_case, non_camel_case_types, dead_code, clippy::upper_case_acronyms)]

use core::ffi::c_void;

pub type HANDLE = isize;
pub type HWND = isize;
pub type HDC = isize;
pub type HMENU = isize;
pub type HBITMAP = isize;
pub type HGDIOBJ = isize;
pub type HFONT = isize;
pub type HINSTANCE = isize;
pub type HCURSOR = isize;
pub type HKEY = isize;
pub type HMONITOR = isize;
pub type HWINEVENTHOOK = isize;
pub type BOOL = i32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type HRESULT = i32;
pub type PCWSTR = *const u16;
pub type PWSTR = *mut u16;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
pub type WINEVENTPROC = unsafe extern "system" fn(HWINEVENTHOOK, u32, HWND, i32, i32, u32, u32);

#[repr(C)] #[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct POINT { pub x: i32, pub y: i32 }
#[repr(C)] #[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct SIZE { pub cx: i32, pub cy: i32 }
#[repr(C)] #[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct RECT { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }
#[repr(C)] #[derive(Clone, Copy, PartialEq, Eq)]
pub struct GUID { pub d1: u32, pub d2: u16, pub d3: u16, pub d4: [u8; 8] }
pub const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> GUID { GUID { d1, d2, d3, d4 } }

#[repr(C)]
pub struct MSG { pub hwnd: HWND, pub message: u32, pub wParam: WPARAM, pub lParam: LPARAM, pub time: u32, pub pt: POINT, pub priv_: u32 }

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32, pub style: u32, pub lpfnWndProc: WNDPROC, pub cbClsExtra: i32, pub cbWndExtra: i32,
    pub hInstance: HINSTANCE, pub hIcon: isize, pub hCursor: HCURSOR, pub hbrBackground: isize,
    pub lpszMenuName: PCWSTR, pub lpszClassName: PCWSTR, pub hIconSm: isize,
}

#[repr(C)] #[derive(Clone, Copy, Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32, pub biWidth: i32, pub biHeight: i32, pub biPlanes: u16, pub biBitCount: u16,
    pub biCompression: u32, pub biSizeImage: u32, pub biXPelsPerMeter: i32, pub biYPelsPerMeter: i32,
    pub biClrUsed: u32, pub biClrImportant: u32,
}
#[repr(C)] #[derive(Clone, Copy, Default)]
pub struct BITMAPINFO { pub bmiHeader: BITMAPINFOHEADER, pub bmiColors: [u32; 1] }
#[repr(C)] #[derive(Clone, Copy)]
pub struct BITMAP { pub bmType: i32, pub bmWidth: i32, pub bmHeight: i32, pub bmWidthBytes: i32, pub bmPlanes: u16, pub bmBitsPixel: u16, pub bmBits: *mut c_void }

#[repr(C)] #[derive(Clone, Copy)]
pub struct BLENDFUNCTION { pub BlendOp: u8, pub BlendFlags: u8, pub SourceConstantAlpha: u8, pub AlphaFormat: u8 }

#[repr(C)]
pub struct TRACKMOUSEEVENT { pub cbSize: u32, pub dwFlags: u32, pub hwndTrack: HWND, pub dwHoverTime: u32 }

#[repr(C)] #[derive(Clone, Copy, Default)]
pub struct FILETIME { pub lo: u32, pub hi: u32 }

#[repr(C)]
pub struct WIN32_FIND_DATAW {
    pub dwFileAttributes: u32, pub ftCreationTime: FILETIME, pub ftLastAccessTime: FILETIME, pub ftLastWriteTime: FILETIME,
    pub nFileSizeHigh: u32, pub nFileSizeLow: u32, pub dwReserved0: u32, pub dwReserved1: u32,
    pub cFileName: [u16; 260], pub cAlternateFileName: [u16; 14],
    pub dwFileType: u32, pub dwCreatorType: u32, pub wFinderFlags: u16,
}

#[repr(C)] #[derive(Default)]
pub struct WIN32_FILE_ATTRIBUTE_DATA { pub attr: u32, pub ctime: FILETIME, pub atime: FILETIME, pub mtime: FILETIME, pub size_hi: u32, pub size_lo: u32 }

#[repr(C)]
pub struct WINDOWPOS { pub hwnd: HWND, pub hwndInsertAfter: HWND, pub x: i32, pub y: i32, pub cx: i32, pub cy: i32, pub flags: u32 }

#[repr(C)] #[derive(Default)]
pub struct MONITORINFO { pub cbSize: u32, pub rcMonitor: RECT, pub rcWork: RECT, pub dwFlags: u32 }

#[repr(C)]
pub struct CHOOSECOLORW {
    pub lStructSize: u32, pub hwndOwner: HWND, pub hInstance: isize, pub rgbResult: u32, pub lpCustColors: *mut u32,
    pub Flags: u32, pub lCustData: isize, pub lpfnHook: usize, pub lpTemplateName: PCWSTR,
}

#[repr(C)]
pub struct OPENFILENAMEW {
    pub lStructSize: u32, pub hwndOwner: HWND, pub hInstance: isize, pub lpstrFilter: PCWSTR,
    pub lpstrCustomFilter: PWSTR, pub nMaxCustFilter: u32, pub nFilterIndex: u32, pub lpstrFile: PWSTR,
    pub nMaxFile: u32, pub lpstrFileTitle: PWSTR, pub nMaxFileTitle: u32, pub lpstrInitialDir: PCWSTR,
    pub lpstrTitle: PCWSTR, pub Flags: u32, pub nFileOffset: u16, pub nFileExtension: u16,
    pub lpstrDefExt: PCWSTR, pub lCustData: isize, pub lpfnHook: usize, pub lpTemplateName: PCWSTR,
    pub pvReserved: *mut c_void, pub dwReserved: u32, pub FlagsEx: u32,
}

#[repr(C)]
pub struct FORMATETC { pub cfFormat: u16, pub ptd: *mut c_void, pub dwAspect: u32, pub lindex: i32, pub tymed: u32 }
#[repr(C)]
pub struct STGMEDIUM { pub tymed: u32, pub data: isize, pub pUnkForRelease: *mut c_void }

#[repr(C)]
pub struct ACCENT_POLICY { pub AccentState: u32, pub AccentFlags: u32, pub GradientColor: u32, pub AnimationId: u32 }
#[repr(C)]
pub struct WINCOMPATTRDATA { pub Attribute: u32, pub Data: *mut c_void, pub SizeOfData: usize }

#[repr(C)]
pub struct DWM_BLURBEHIND { pub dwFlags: u32, pub fEnable: BOOL, pub hRgnBlur: isize, pub fTransitionOnMaximized: BOOL }

#[repr(C)]
pub struct SHELLEXECUTEINFOW {
    pub cbSize: u32, pub fMask: u32, pub hwnd: HWND, pub lpVerb: PCWSTR, pub lpFile: PCWSTR, pub lpParameters: PCWSTR,
    pub lpDirectory: PCWSTR, pub nShow: i32, pub hInstApp: isize, pub lpIDList: *mut c_void, pub lpClass: PCWSTR,
    pub hkeyClass: HKEY, pub dwHotKey: u32, pub hIcon: HANDLE, pub hProcess: HANDLE,
}

#[repr(C)]
pub struct SHFILEOPSTRUCTW {
    pub hwnd: HWND, pub wFunc: u32, pub pFrom: PCWSTR, pub pTo: PCWSTR, pub fFlags: u16,
    pub fAnyOperationsAborted: BOOL, pub hNameMappings: *mut c_void, pub lpszProgressTitle: PCWSTR,
}
pub const FO_DELETE: u32 = 3;
pub const FOF_SILENT: u16 = 0x4;
pub const FOF_NOCONFIRMATION: u16 = 0x10;
pub const FOF_ALLOWUNDO: u16 = 0x40;
pub const FOF_NOERRORUI: u16 = 0x400;
pub const HTCLIENT: isize = 1;
pub const HTCAPTION: isize = 2;
pub const WS_EX_APPWINDOW: u32 = 0x40000;
pub const SYNCHRONIZE: u32 = 0x100000;
pub const REG_DWORD: u32 = 4;
pub const MOVEFILE_REPLACE_EXISTING: u32 = 1;
pub const WM_ACTIVATE: u32 = 0x06;
pub const WM_KILLFOCUS: u32 = 0x08;
pub const WM_SYSKEYDOWN: u32 = 0x104;
pub const WM_MOUSEHWHEEL: u32 = 0x20E;
pub const WA_INACTIVE: usize = 0;
pub const MA_ACTIVATE: isize = 1;
pub const IDC_IBEAM: usize = 32513;
pub const VK_BACK: usize = 0x08;
pub const VK_TAB: usize = 0x09;
pub const VK_END: usize = 0x23;
pub const VK_HOME: usize = 0x24;
pub const VK_LEFT: usize = 0x25;
pub const VK_UP: usize = 0x26;
pub const VK_RIGHT: usize = 0x27;
pub const VK_DOWN: usize = 0x28;
pub const VK_DELETE: usize = 0x2E;
pub const VK_LBUTTON: i32 = 0x01;
pub const CF_UNICODETEXT: u32 = 13;
pub const GMEM_MOVEABLE: u32 = 2;
pub const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;
pub const SEE_MASK_NOASYNC: u32 = 0x100;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const TOKEN_QUERY: u32 = 8;

// ---------------------------------------------------------------- constants
pub const HEAP_ZERO_MEMORY: u32 = 8;
pub const GENERIC_READ: u32 = 0x8000_0000;
pub const GENERIC_WRITE: u32 = 0x4000_0000;
pub const FILE_SHARE_READ: u32 = 1;
pub const FILE_SHARE_WRITE: u32 = 2;
pub const FILE_SHARE_DELETE: u32 = 4;
pub const CREATE_ALWAYS: u32 = 2;
pub const OPEN_EXISTING: u32 = 3;
pub const INVALID_HANDLE_VALUE: isize = -1;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
pub const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
pub const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
pub const MOVEFILE_COPY_ALLOWED: u32 = 2;
pub const FILE_NOTIFY_CHANGE_LAST_WRITE: u32 = 0x10;
pub const FILE_NOTIFY_CHANGE_FILE_NAME: u32 = 0x1;
pub const PAGE_READONLY: u32 = 2;
pub const FILE_MAP_READ: u32 = 4;
pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const WAIT_OBJECT_0: u32 = 0;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const QS_ALLINPUT: u32 = 0x04FF;
pub const MWMO_INPUTAVAILABLE: u32 = 4;
pub const PM_REMOVE: u32 = 1;
pub const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;

pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_TOOLWINDOW: u32 = 0x80;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const WS_EX_TOPMOST: u32 = 0x8;
pub const WS_EX_TRANSPARENT: u32 = 0x20;
pub const WS_EX_DLGMODALFRAME: u32 = 1;
pub const ES_AUTOHSCROLL: u32 = 0x80;
pub const GWL_EXSTYLE: i32 = -20;
pub const GWLP_USERDATA: i32 = -21;
pub const HWND_MESSAGE: HWND = -3;
pub const HWND_TOP: HWND = 0;
pub const HWND_BOTTOM: HWND = 1;
pub const HWND_TOPMOST: HWND = -1;
pub const HWND_NOTOPMOST: HWND = -2;
pub const SWP_NOSIZE: u32 = 1;
pub const SWP_NOMOVE: u32 = 2;
pub const SWP_NOZORDER: u32 = 4;
pub const SWP_NOACTIVATE: u32 = 0x10;
pub const SWP_SHOWWINDOW: u32 = 0x40;
pub const SWP_NOOWNERZORDER: u32 = 0x200;
pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const SW_SHOW: i32 = 5;
pub const GW_HWNDPREV: u32 = 3;
pub const GW_HWNDNEXT: u32 = 2;
pub const ULW_ALPHA: u32 = 2;
pub const AC_SRC_ALPHA: u8 = 1;
pub const CS_DBLCLKS: u32 = 8;
pub const IDC_ARROW: usize = 32512;
pub const IDC_HAND: usize = 32649;
pub const IDC_SIZEALL: usize = 32646;
pub const IDC_SIZENWSE: usize = 32642;

pub const WM_CREATE: u32 = 0x01;
pub const WM_DESTROY: u32 = 0x02;
pub const WM_MOVE: u32 = 0x03;
pub const WM_SIZE: u32 = 0x05;
pub const WM_SETFOCUS: u32 = 0x07;
pub const WM_CLOSE: u32 = 0x10;
pub const WM_QUIT: u32 = 0x12;
pub const WM_SETCURSOR: u32 = 0x20;
pub const WM_MOUSEACTIVATE: u32 = 0x21;
pub const WM_WINDOWPOSCHANGING: u32 = 0x46;
pub const WM_SETFONT: u32 = 0x30;
pub const WM_NCHITTEST: u32 = 0x84;
pub const WM_KEYDOWN: u32 = 0x100;
pub const WM_CHAR: u32 = 0x102;
pub const WM_COMMAND: u32 = 0x111;
pub const WM_TIMER: u32 = 0x113;
pub const WM_MOUSEMOVE: u32 = 0x200;
pub const WM_LBUTTONDOWN: u32 = 0x201;
pub const WM_LBUTTONUP: u32 = 0x202;
pub const WM_LBUTTONDBLCLK: u32 = 0x203;
pub const WM_RBUTTONDOWN: u32 = 0x204;
pub const WM_RBUTTONUP: u32 = 0x205;
pub const WM_MBUTTONUP: u32 = 0x208;
pub const WM_MOUSEWHEEL: u32 = 0x20A;
pub const WM_CAPTURECHANGED: u32 = 0x215;
pub const WM_MOUSELEAVE: u32 = 0x2A3;
pub const WM_DPICHANGED: u32 = 0x2E0;
pub const WM_DISPLAYCHANGE: u32 = 0x7E;
pub const WM_SETTINGCHANGE: u32 = 0x1A;
pub const WM_USER: u32 = 0x400;
pub const MA_NOACTIVATE: isize = 3;
pub const TME_LEAVE: u32 = 2;
pub const VK_RETURN: usize = 0x0D;
pub const VK_ESCAPE: usize = 0x1B;
pub const VK_SHIFT: i32 = 0x10;
pub const VK_CONTROL: i32 = 0x11;
pub const MK_LBUTTON: usize = 1;

pub const MF_STRING: u32 = 0;
pub const MF_GRAYED: u32 = 1;
pub const MF_CHECKED: u32 = 8;
pub const MF_POPUP: u32 = 0x10;
pub const MF_SEPARATOR: u32 = 0x800;
pub const MF_MENUBREAK: u32 = 0x40;
pub const TPM_RETURNCMD: u32 = 0x100;
pub const TPM_RIGHTBUTTON: u32 = 2;
pub const TPM_NONOTIFY: u32 = 0x80;

pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
pub const SPI_GETWORKAREA: u32 = 0x30;
pub const SM_CXDRAG: i32 = 68;
pub const SM_CYDRAG: i32 = 69;
pub const EVENT_SYSTEM_FOREGROUND: u32 = 3;
pub const WINEVENT_OUTOFCONTEXT: u32 = 0;
pub const WINEVENT_SKIPOWNPROCESS: u32 = 2;
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

pub const DIB_RGB_COLORS: u32 = 0;
pub const BI_RGB: u32 = 0;
pub const TRANSPARENT: i32 = 1;
pub const ANTIALIASED_QUALITY: u32 = 4;
pub const DT_CENTER: u32 = 1;
pub const DT_LEFT: u32 = 0;
pub const DT_VCENTER: u32 = 4;
pub const DT_SINGLELINE: u32 = 0x20;
pub const DT_WORDBREAK: u32 = 0x10;
pub const DT_NOPREFIX: u32 = 0x800;
pub const DT_CALCRECT: u32 = 0x400;
pub const DT_END_ELLIPSIS: u32 = 0x8000;
pub const DT_EDITCONTROL: u32 = 0x2000;
pub const DT_WORD_ELLIPSIS: u32 = 0x40000;
pub const FW_NORMAL: i32 = 400;
pub const FW_SEMIBOLD: i32 = 600;
pub const DEFAULT_CHARSET: u32 = 1;

pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as i32 as isize;
pub const KEY_READ: u32 = 0x20019;
pub const KEY_WRITE: u32 = 0x20006;
pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;

pub const COINIT_APARTMENTTHREADED: u32 = 2;
pub const CLSCTX_INPROC_SERVER: u32 = 1;
pub const CF_HDROP: u16 = 15;
pub const DVASPECT_CONTENT: u32 = 1;
pub const TYMED_HGLOBAL: u32 = 1;
pub const DROPEFFECT_NONE: u32 = 0;
pub const DROPEFFECT_COPY: u32 = 1;
pub const DROPEFFECT_MOVE: u32 = 2;
pub const DROPEFFECT_LINK: u32 = 4;
pub const S_OK: HRESULT = 0;
pub const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as i32;

pub const CC_RGBINIT: u32 = 1;
pub const CC_FULLOPEN: u32 = 2;
pub const OFN_FILEMUSTEXIST: u32 = 0x1000;
pub const OFN_PATHMUSTEXIST: u32 = 0x800;
pub const OFN_NOCHANGEDIR: u32 = 8;
pub const OFN_EXPLORER: u32 = 0x80000;
pub const EM_SETSEL: u32 = 0xB1;
pub const EM_LIMITTEXT: u32 = 0xC5;
pub const MB_OK: u32 = 0;
pub const MB_YESNO: u32 = 4;
pub const MB_ICONWARNING: u32 = 0x30;
pub const MB_ICONINFORMATION: u32 = 0x40;
pub const IDYES: i32 = 6;

// --------------------------------------------------------------- functions
#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    pub fn GetModuleHandleW(n: PCWSTR) -> HINSTANCE;
    pub fn GetModuleFileNameW(m: HINSTANCE, buf: PWSTR, n: u32) -> u32;
    pub fn ExitProcess(code: u32) -> !;
    pub fn GetProcessHeap() -> HANDLE;
    pub fn HeapAlloc(h: HANDLE, f: u32, n: usize) -> *mut c_void;
    pub fn HeapReAlloc(h: HANDLE, f: u32, p: *mut c_void, n: usize) -> *mut c_void;
    pub fn HeapFree(h: HANDLE, f: u32, p: *mut c_void) -> BOOL;
    pub fn HeapCompact(h: HANDLE, f: u32) -> usize;
    pub fn GetEnvironmentVariableW(n: PCWSTR, buf: PWSTR, sz: u32) -> u32;
    pub fn ExpandEnvironmentStringsW(src: PCWSTR, dst: PWSTR, n: u32) -> u32;
    pub fn CreateFileW(n: PCWSTR, acc: u32, share: u32, sa: *const c_void, disp: u32, flags: u32, tmpl: HANDLE) -> HANDLE;
    pub fn ReadFile(h: HANDLE, buf: *mut c_void, n: u32, read: *mut u32, ov: *mut c_void) -> BOOL;
    pub fn WriteFile(h: HANDLE, buf: *const c_void, n: u32, written: *mut u32, ov: *mut c_void) -> BOOL;
    pub fn GetFileSizeEx(h: HANDLE, sz: *mut i64) -> BOOL;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn CreateDirectoryW(n: PCWSTR, sa: *const c_void) -> BOOL;
    pub fn RemoveDirectoryW(n: PCWSTR) -> BOOL;
    pub fn MoveFileExW(a: PCWSTR, b: PCWSTR, f: u32) -> BOOL;
    pub fn CopyFileW(a: PCWSTR, b: PCWSTR, fail_if_exists: BOOL) -> BOOL;
    pub fn DeleteFileW(n: PCWSTR) -> BOOL;
    pub fn FindFirstFileW(n: PCWSTR, d: *mut WIN32_FIND_DATAW) -> HANDLE;
    pub fn FindNextFileW(h: HANDLE, d: *mut WIN32_FIND_DATAW) -> BOOL;
    pub fn FindClose(h: HANDLE) -> BOOL;
    pub fn GetFileAttributesW(n: PCWSTR) -> u32;
    pub fn GetFileAttributesExW(n: PCWSTR, lvl: u32, d: *mut WIN32_FILE_ATTRIBUTE_DATA) -> BOOL;
    pub fn SetFileAttributesW(n: PCWSTR, a: u32) -> BOOL;
    pub fn QueryPerformanceCounter(c: *mut i64) -> BOOL;
    pub fn QueryPerformanceFrequency(f: *mut i64) -> BOOL;
    pub fn CreateMutexW(sa: *const c_void, own: BOOL, n: PCWSTR) -> HANDLE;
    pub fn GetLastError() -> u32;
    pub fn FindFirstChangeNotificationW(p: PCWSTR, sub: BOOL, filter: u32) -> HANDLE;
    pub fn FindNextChangeNotification(h: HANDLE) -> BOOL;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn K32EmptyWorkingSet(p: HANDLE) -> BOOL;
    pub fn K32GetProcessMemoryInfo(p: HANDLE, c: *mut PROCESS_MEMORY_COUNTERS, cb: u32) -> BOOL;
    pub fn CreateFileMappingW(h: HANDLE, sa: *const c_void, prot: u32, hi: u32, lo: u32, n: PCWSTR) -> HANDLE;
    pub fn MapViewOfFile(h: HANDLE, acc: u32, hi: u32, lo: u32, n: usize) -> *mut c_void;
    pub fn UnmapViewOfFile(p: *const c_void) -> BOOL;
    pub fn Sleep(ms: u32);
    pub fn GetTickCount() -> u32;
    pub fn SetProcessInformation(p: HANDLE, class: u32, info: *const c_void, sz: u32) -> BOOL;
    pub fn GetCommandLineW() -> PCWSTR;
    pub fn GetUserDefaultUILanguage() -> u16;
    pub fn FindResourceW(m: HINSTANCE, name: usize, ty: usize) -> HANDLE;
    pub fn LoadResource(m: HINSTANCE, r: HANDLE) -> HANDLE;
    pub fn LockResource(h: HANDLE) -> *const u8;
    pub fn SizeofResource(m: HINSTANCE, r: HANDLE) -> u32;
    pub fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
    pub fn GetTempPathW(n: u32, buf: PWSTR) -> u32;
    pub fn SetDefaultDllDirectories(flags: u32) -> BOOL;
    pub fn SetProcessMitigationPolicy(policy: u32, buf: *const c_void, len: usize) -> BOOL;
    pub fn GetComputerNameW(b: PWSTR, n: *mut u32) -> BOOL;
    pub fn GlobalAlloc(f: u32, n: usize) -> isize;
    pub fn GlobalFree(h: isize) -> isize;
    pub fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
    pub fn GetExitCodeProcess(h: HANDLE, code: *mut u32) -> BOOL;
    pub fn LocalFree(h: isize) -> isize;
    pub fn GlobalLock(h: isize) -> *mut c_void;
    pub fn GlobalUnlock(h: isize) -> BOOL;
}

#[link(name = "user32", kind = "raw-dylib")]
extern "system" {
    pub fn RegisterClassExW(c: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(ex: u32, cls: PCWSTR, name: PCWSTR, style: u32, x: i32, y: i32, w: i32, h: i32,
                           parent: HWND, menu: HMENU, inst: HINSTANCE, param: *const c_void) -> HWND;
    pub fn DestroyWindow(h: HWND) -> BOOL;
    pub fn DefWindowProcW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn GetMessageW(m: *mut MSG, h: HWND, a: u32, b: u32) -> BOOL;
    pub fn PeekMessageW(m: *mut MSG, h: HWND, a: u32, b: u32, r: u32) -> BOOL;
    pub fn TranslateMessage(m: *const MSG) -> BOOL;
    pub fn DispatchMessageW(m: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(code: i32);
    pub fn PostMessageW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> BOOL;
    pub fn SendMessageW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn MsgWaitForMultipleObjectsEx(n: u32, h: *const HANDLE, ms: u32, mask: u32, f: u32) -> u32;
    pub fn ShowWindow(h: HWND, cmd: i32) -> BOOL;
    pub fn SetWindowPos(h: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, f: u32) -> BOOL;
    pub fn GetWindowRect(h: HWND, r: *mut RECT) -> BOOL;
    pub fn UpdateLayeredWindow(h: HWND, dst: HDC, pos: *const POINT, sz: *const SIZE, src: HDC, spt: *const POINT,
                               key: u32, blend: *const BLENDFUNCTION, f: u32) -> BOOL;
    pub fn SetWindowLongPtrW(h: HWND, i: i32, v: isize) -> isize;
    pub fn GetWindowLongPtrW(h: HWND, i: i32) -> isize;
    pub fn LoadCursorW(i: HINSTANCE, n: usize) -> HCURSOR;
    pub fn SetCursor(c: HCURSOR) -> HCURSOR;
    pub fn TrackMouseEvent(t: *mut TRACKMOUSEEVENT) -> BOOL;
    pub fn SetCapture(h: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn GetCapture() -> HWND;
    pub fn GetCursorPos(p: *mut POINT) -> BOOL;
    pub fn WindowFromPoint(p: POINT) -> HWND;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(m: HMENU, f: u32, id: usize, s: PCWSTR) -> BOOL;
    pub fn TrackPopupMenu(m: HMENU, f: u32, x: i32, y: i32, r: i32, h: HWND, rc: *const RECT) -> i32;
    pub fn DestroyMenu(m: HMENU) -> BOOL;
    pub fn SetForegroundWindow(h: HWND) -> BOOL;
    pub fn GetForegroundWindow() -> HWND;
    pub fn MonitorFromPoint(p: POINT, f: u32) -> HMONITOR;
    pub fn MonitorFromWindow(h: HWND, f: u32) -> HMONITOR;
    pub fn GetMonitorInfoW(m: HMONITOR, i: *mut MONITORINFO) -> BOOL;
    pub fn GetDpiForWindow(h: HWND) -> u32;
    pub fn GetDpiForSystem() -> u32;
    pub fn SetProcessDpiAwarenessContext(c: isize) -> BOOL;
    pub fn FindWindowW(cls: PCWSTR, name: PCWSTR) -> HWND;
    pub fn FindWindowExW(parent: HWND, after: HWND, cls: PCWSTR, name: PCWSTR) -> HWND;
    pub fn GetWindow(h: HWND, cmd: u32) -> HWND;
    pub fn GetClassNameW(h: HWND, buf: PWSTR, n: i32) -> i32;
    pub fn SetWinEventHook(min: u32, max: u32, m: HINSTANCE, f: WINEVENTPROC, pid: u32, tid: u32, flags: u32) -> HWINEVENTHOOK;
    pub fn RegisterWindowMessageW(s: PCWSTR) -> u32;
    pub fn SetTimer(h: HWND, id: usize, ms: u32, f: usize) -> usize;
    pub fn KillTimer(h: HWND, id: usize) -> BOOL;
    pub fn GetDC(h: HWND) -> HDC;
    pub fn ReleaseDC(h: HWND, dc: HDC) -> i32;
    pub fn MessageBoxW(h: HWND, t: PCWSTR, c: PCWSTR, f: u32) -> i32;
    pub fn IsWindow(h: HWND) -> BOOL;
    pub fn GetSystemMetrics(i: i32) -> i32;
    pub fn SystemParametersInfoW(a: u32, b: u32, p: *mut c_void, f: u32) -> BOOL;
    pub fn GetDoubleClickTime() -> u32;
    pub fn GetKeyState(k: i32) -> i16;
    pub fn GetAsyncKeyState(k: i32) -> i16;
    pub fn LoadIconW(i: HINSTANCE, n: usize) -> isize;
    pub fn OpenClipboard(h: HWND) -> BOOL;
    pub fn CloseClipboard() -> BOOL;
    pub fn EmptyClipboard() -> BOOL;
    pub fn GetClipboardData(f: u32) -> isize;
    pub fn SetClipboardData(f: u32, h: isize) -> isize;
    pub fn GetCaretBlinkTime() -> u32;
    pub fn GetFocus() -> HWND;
    pub fn IsWindowVisible(h: HWND) -> BOOL;
    pub fn SetFocus(h: HWND) -> HWND;
    pub fn GetWindowTextW(h: HWND, b: PWSTR, n: i32) -> i32;
    pub fn SetWindowTextW(h: HWND, s: PCWSTR) -> BOOL;
    pub fn GetWindowTextLengthW(h: HWND) -> i32;
    pub fn DrawTextW(dc: HDC, s: PCWSTR, n: i32, r: *mut RECT, f: u32) -> i32;
    pub fn IsDialogMessageW(h: HWND, m: *mut MSG) -> BOOL;
    pub fn GetWindowThreadProcessId(h: HWND, pid: *mut u32) -> u32;
    pub fn AllowSetForegroundWindow(pid: u32) -> BOOL;
    pub fn SetWindowRgn(h: HWND, rgn: isize, redraw: BOOL) -> i32;
    pub fn AdjustWindowRectEx(r: *mut RECT, style: u32, menu: BOOL, ex: u32) -> BOOL;
}

#[link(name = "gdi32", kind = "raw-dylib")]
extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn CreateDIBSection(dc: HDC, bi: *const BITMAPINFO, usage: u32, bits: *mut *mut c_void, sec: HANDLE, off: u32) -> HBITMAP;
    pub fn SelectObject(dc: HDC, o: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(o: HGDIOBJ) -> BOOL;
    pub fn CreateFontW(h: i32, w: i32, esc: i32, ori: i32, weight: i32, italic: u32, underline: u32, strike: u32,
                       charset: u32, out: u32, clip: u32, quality: u32, pitch: u32, face: PCWSTR) -> HFONT;
    pub fn SetTextColor(dc: HDC, c: u32) -> u32;
    pub fn SetBkMode(dc: HDC, m: i32) -> i32;
    pub fn GdiFlush() -> BOOL;
    pub fn GetTextExtentExPointW(dc: HDC, s: PCWSTR, n: i32, max: i32, fit: *mut i32, dx: *mut i32, sz: *mut SIZE) -> BOOL;
    pub fn GetDIBits(dc: HDC, bm: HBITMAP, start: u32, lines: u32, bits: *mut c_void, bi: *mut BITMAPINFO, usage: u32) -> i32;
    pub fn GetObjectW(h: HGDIOBJ, n: i32, p: *mut c_void) -> i32;
    pub fn GetStockObject(i: i32) -> HGDIOBJ;
    pub fn CreateRoundRectRgn(a: i32, b: i32, c: i32, d: i32, w: i32, h: i32) -> isize;
}

#[link(name = "ole32", kind = "raw-dylib")]
extern "system" {
    pub fn OleInitialize(r: *mut c_void) -> HRESULT;
    pub fn RegisterDragDrop(h: HWND, t: *mut c_void) -> HRESULT;
    pub fn RevokeDragDrop(h: HWND) -> HRESULT;
    pub fn ReleaseStgMedium(m: *mut STGMEDIUM);
    pub fn CoCreateInstance(clsid: *const GUID, outer: *mut c_void, ctx: u32, iid: *const GUID, out: *mut *mut c_void) -> HRESULT;
}

#[link(name = "advapi32", kind = "raw-dylib")]
extern "system" {
    pub fn RegOpenKeyExW(k: HKEY, sub: PCWSTR, opt: u32, sam: u32, out: *mut HKEY) -> i32;
    pub fn RegQueryValueExW(k: HKEY, n: PCWSTR, r: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    pub fn RegSetValueExW(k: HKEY, n: PCWSTR, r: u32, ty: u32, data: *const u8, len: u32) -> i32;
    pub fn RegCreateKeyExW(k: HKEY, sub: PCWSTR, r: u32, cls: PCWSTR, opt: u32, sam: u32, sa: *const c_void, out: *mut HKEY, disp: *mut u32) -> i32;
    pub fn RegDeleteTreeW(k: HKEY, sub: PCWSTR) -> i32;
    pub fn RegDeleteValueW(k: HKEY, n: PCWSTR) -> i32;
    pub fn RegCloseKey(k: HKEY) -> i32;
    pub fn OpenProcessToken(p: HANDLE, access: u32, out: *mut HANDLE) -> BOOL;
    pub fn GetTokenInformation(t: HANDLE, class: u32, buf: *mut c_void, len: u32, ret: *mut u32) -> BOOL;
    pub fn ConvertSidToStringSidW(sid: *mut c_void, out: *mut PWSTR) -> BOOL;
}

// ---------------------------------------------- shell, dialogs and DWM
// Plain imports. Resolving functions by name at run time is a pattern antivirus
// heuristics associate with malware, so every function is imported normally.
#[repr(C)]
pub struct MARGINS { pub l: i32, pub r: i32, pub t: i32, pub b: i32 }

#[link(name = "dwmapi", kind = "raw-dylib")]
extern "system" {
    pub fn DwmFlush() -> HRESULT;
    pub fn DwmEnableBlurBehindWindow(h: HWND, b: *const DWM_BLURBEHIND) -> HRESULT;
    pub fn DwmExtendFrameIntoClientArea(h: HWND, m: *const MARGINS) -> HRESULT;
    pub fn DwmSetWindowAttribute(h: HWND, a: u32, v: *const c_void, n: u32) -> HRESULT;
}

#[link(name = "user32", kind = "raw-dylib")]
extern "system" {
    pub fn SetWindowCompositionAttribute(h: HWND, d: *mut WINCOMPATTRDATA) -> BOOL;
}

#[link(name = "shell32", kind = "raw-dylib")]
extern "system" {
    pub fn SHFileOperationW(op: *mut SHFILEOPSTRUCTW) -> i32;
    pub fn DragQueryFileW(h: isize, i: u32, buf: PWSTR, n: u32) -> u32;
    pub fn ShellExecuteW(h: HWND, op: PCWSTR, file: PCWSTR, params: PCWSTR, dir: PCWSTR, show: i32) -> isize;
    pub fn SHCreateItemFromParsingName(p: PCWSTR, bc: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT;
    pub fn SHChangeNotify(event: i32, flags: u32, a: *const c_void, b: *const c_void);
}

#[link(name = "comdlg32", kind = "raw-dylib")]
extern "system" {
    pub fn GetOpenFileNameW(o: *mut OPENFILENAMEW) -> BOOL;
}

// ------------------------------------------------------------- COM helpers
/// Calls vtable slot `idx` of COM object `obj`.
#[inline(always)]
pub unsafe fn vt(obj: *mut c_void, idx: usize) -> usize {
    *(*(obj as *const *const usize)).add(idx)
}
pub unsafe fn com_release(obj: *mut c_void) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 = core::mem::transmute(vt(obj, 2));
        f(obj);
    }
}

#[repr(C)]
#[derive(Default)]
pub struct PROCESS_MEMORY_COUNTERS {
    pub cb: u32,
    pub PageFaultCount: u32,
    pub PeakWorkingSetSize: usize,
    pub WorkingSetSize: usize,
    pub QuotaPeakPagedPoolUsage: usize,
    pub QuotaPagedPoolUsage: usize,
    pub QuotaPeakNonPagedPoolUsage: usize,
    pub QuotaNonPagedPoolUsage: usize,
    pub PagefileUsage: usize,
    pub PeakPagefileUsage: usize,
}
