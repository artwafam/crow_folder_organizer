//! The Crow app: entry point, command line and the message loop.
#![no_std]
#![no_main]

use crow::ffi::*;
use crow::*;


#[no_mangle]
pub extern "system" fn entry() -> ! {
    unsafe {
        harden();
        // Command line options. --new and --new-panel come from the desktop context
        // menu, and --export-art <dir> is used when packaging.
        let cl = util::from_w(core::slice::from_raw_parts(GetCommandLineW(), 32768.min(rt::wcslen(GetCommandLineW()))));
        if let Some(p) = cl.find("--snapshot") {
            let dir = cl[p + 10..].trim().trim_matches('"');
            OleInitialize(core::ptr::null_mut());
            ExitProcess(if snapshot::run(&util::to_w(dir)) { 0 } else { 1 });
        }
        if let Some(p) = cl.find("--export-art") {
            let dir = cl[p + 12..].trim().trim_matches('"');
            ExitProcess(if art::export(&util::to_w(dir)) { 0 } else { 1 });
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if cl.contains("--uninstall") {
            OleInitialize(core::ptr::null_mut());
            installer::uninstall();
            ExitProcess(0);
        }
        let new_req: usize = if cl.contains("--new-panel") { 2 } else if cl.contains("--new") { 1 } else { 0 };
        // Started from the Start menu (no options) or with --settings: show the
        // settings window. Windows sign-in uses --background and shows nothing.
        let want_prefs = cl.contains("--settings") || (new_req == 0 && !cl.contains("--background"));

        // Only one instance runs. A second one forwards its request and exits.
        CreateMutexW(core::ptr::null(), 1, w!("Local\\CrowFolderOrganizer").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let h = FindWindowW(w!("CrowFOHost").as_ptr(), core::ptr::null());
            if h != 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, &mut pid);
                AllowSetForegroundWindow(pid); // let it bring its window to the front
                if new_req != 0 {
                    PostMessageW(h, app::WM_CROW_NEW, new_req, 0);
                } else if want_prefs {
                    PostMessageW(h, app::WM_CROW_PREFS, 0, 0);
                }
            }
            ExitProcess(0);
        }
        OleInitialize(core::ptr::null_mut());

        let a = app::App::init();
        if new_req != 0 {
            a.new_folder_at_cursor(new_req == 2);
        } else if want_prefs {
            a.open_prefs();
        }

        // wake up when crow.ini or any folder changes on disk (hot reload, OneDrive sync)
        let root = util::wz(&store::root());
        let watch = FindFirstChangeNotificationW(root.as_ptr(), 1, FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME | 0x2 /*DIR_NAME*/);
        let handles = [watch];
        let nh = if watch == INVALID_HANDLE_VALUE { 0 } else { 1 };

        let mut msg: MSG = core::mem::zeroed();
        loop {
            if a.any_animating() {
                // While animating, render once per compositor frame so it follows vsync
                while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        quit();
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                a.tick();
                if DwmFlush() < 0 {
                    Sleep(8);
                }
                continue;
            }
            a.was_animating = false;
            // When idle, wait without using CPU until input, a timer or a file change arrives
            let r = MsgWaitForMultipleObjectsEx(nh, handles.as_ptr(), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            if nh == 1 && r == WAIT_OBJECT_0 {
                FindNextChangeNotification(watch);
                a.reload();
            }
            while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    quit();
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // Anything that woke us up (a window switch, a timer, Windows itself)
            // touches memory again. Trim it once things are quiet, unless this
            // wakeup was the trim itself.
            if a.trimmed {
                a.trimmed = false;
            } else {
                a.arm_idle();
            }
        }
    }
}

/// Process hardening, applied before anything else runs.
/// DLLs are only searched in System32 and the app folder, never in the current
/// directory or PATH, which blocks DLL planting. Images from network shares or
/// low integrity locations can't be loaded, and legacy injection points such as
/// AppInit DLLs, global hook DLLs and LSPs are turned off.
unsafe fn harden() {
    SetDefaultDllDirectories(0x1000 /*LOAD_LIBRARY_SEARCH_DEFAULT_DIRS*/);
    let image_load: u32 = 1 | 2 | 4; // NoRemoteImages | NoLowMandatoryLabelImages | PreferSystem32Images
    SetProcessMitigationPolicy(10 /*ProcessImageLoadPolicy*/, &image_load as *const u32 as _, 4);
    let ext_points: u32 = 1; // DisableExtensionPoints
    SetProcessMitigationPolicy(6 /*ProcessExtensionPointDisablePolicy*/, &ext_points as *const u32 as _, 4);
}

fn quit() -> ! {
    let a = app::app();
    a.save();
    unsafe {
        for w in &a.wins {
            RevokeDragDrop(w.hwnd);
        }
        ExitProcess(0)
    }
}
