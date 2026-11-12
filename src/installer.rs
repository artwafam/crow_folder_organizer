//! Installer and uninstaller. CrowFOSetup.exe carries CrowFO.exe inside it and
//! installs it for the current user only (no admin rights): the app goes to
//! %LOCALAPPDATA%\Programs\Crow FO, a Start menu shortcut is created and it is
//! listed in "Installed apps" so it can be removed like any other program.
//! `CrowFO.exe --uninstall` shows the matching uninstall window, and the same
//! window offers new versions found by the update check (see update.rs).

use crate::app::{Fonts, WM_CROW_QUIT};
use crate::ffi::*;
use crate::gfx::*;
use crate::i18n::tr;
use crate::store;
use crate::uikit::*;
use crate::util::*;
use alloc::string::String;

const UNINSTALL_KEY: &[u16] = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CrowFO");
const MARGIN: f32 = 18.0;
const BAR: f32 = 44.0;

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Ask,
    /// Downloading an update.
    Working,
    Done,
    Failed,
}

/// Posted by the download thread when it is done, with 1 in wParam on success.
const WM_UPDATE_DONE: u32 = 0x8000 + 1;

struct Dlg {
    hwnd: HWND,
    uninstall: bool,
    /// The tag of the new release when this window offers an update.
    update: Option<String>,
    payload: &'static [u8],
    sc: f32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    cv: Option<Canvas>,
    texts: Texts,
    icons: Icons,
    anim: Anim,
    fonts: Fonts,
    inp: Input,
    active: u32,
    cursor: usize,
    step: Step,
    startup: bool,
    menu: bool,
    updates: bool,
    restore: bool,
    updating: bool,
    quit: bool,
}

static mut DLG: *mut Dlg = core::ptr::null_mut();

/// Process hardening shared by CrowFO.exe and the installer: DLLs only from
/// System32 and the app folder, no images from network shares or low
/// integrity locations, and no legacy injection points.
pub unsafe fn harden() {
    SetDefaultDllDirectories(0x1000 /*LOAD_LIBRARY_SEARCH_DEFAULT_DIRS*/);
    let image_load: u32 = 1 | 2 | 4;
    SetProcessMitigationPolicy(10, &image_load as *const u32 as _, 4);
    let ext_points: u32 = 1;
    SetProcessMitigationPolicy(6, &ext_points as *const u32 as _, 4);
}

pub fn install_dir() -> WString {
    let base = env(w!("LOCALAPPDATA"));
    wcat(&[&base, w!("\\Programs\\Crow FO")])
}
fn start_menu_link() -> WString {
    wcat(&[&env(w!("APPDATA")), w!("\\Microsoft\\Windows\\Start Menu\\Programs\\Crow FO.lnk")])
}

/// Asks a running Crow to save and quit, and waits for it.
fn quit_running() {
    unsafe {
        let h = FindWindowW(w!("CrowFOHost").as_ptr(), core::ptr::null());
        if h == 0 {
            return;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        let proc_ = OpenProcess(SYNCHRONIZE, 0, pid);
        PostMessageW(h, WM_CROW_QUIT, 0, 0);
        if proc_ != 0 {
            WaitForSingleObject(proc_, 5000);
            CloseHandle(proc_);
        } else {
            Sleep(800);
        }
    }
}

fn installed_version() -> Option<String> {
    unsafe {
        let mut k: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, UNINSTALL_KEY.as_ptr(), 0, KEY_READ, &mut k) != 0 {
            return None;
        }
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = 0;
        let r = RegQueryValueExW(k, w!("DisplayVersion").as_ptr(), core::ptr::null_mut(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut len);
        RegCloseKey(k);
        if r == 0 { Some(from_w(&buf)) } else { None }
    }
}

/// `updates` is None to keep the current update check setting.
fn install(payload: &[u8], startup: bool, menu: bool, updates: Option<bool>) -> bool {
    quit_running();
    let base = wcat(&[&env(w!("LOCALAPPDATA")), w!("\\Programs")]);
    mkdir(&base);
    let dir = install_dir();
    mkdir(&dir);
    let exe = wcat(&[&dir, w!("\\CrowFO.exe")]);
    // the old process may need a moment to let go of its file
    let mut ok = false;
    for _ in 0..20 {
        if write_file(&exe, payload) {
            ok = true;
            break;
        }
        unsafe { Sleep(200) };
    }
    if !ok {
        return false;
    }
    unsafe { store::create_link(&start_menu_link(), &exe) };
    let key = UNINSTALL_KEY;
    store::reg_str(key, Some(w!("DisplayName")), w!("Crow FO"));
    store::reg_str(key, Some(w!("DisplayVersion")), &to_w(env!("CROW_VERSION")));
    store::reg_str(key, Some(w!("Publisher")), &to_w(env!("CARGO_PKG_AUTHORS")));
    store::reg_str(key, Some(w!("DisplayIcon")), &wcat(&[&exe, w!(",0")]));
    store::reg_str(key, Some(w!("InstallLocation")), &dir);
    store::reg_str(key, Some(w!("UninstallString")), &wcat(&[w!("\""), &exe, w!("\" --uninstall")]));
    store::reg_str(key, Some(w!("URLInfoAbout")), &to_w(env!("CARGO_PKG_REPOSITORY")));
    store::reg_dword(key, w!("NoModify"), 1);
    store::reg_dword(key, w!("NoRepair"), 1);
    store::reg_dword(key, w!("EstimatedSize"), (payload.len() / 1024) as u32 + 1);
    store::set_autostart_for(&exe, startup);
    store::set_desktop_menu_for(&exe, menu);
    if let Some(on) = updates {
        crate::update::set_enabled(on);
    }
    true
}

fn remove_tree(dir: &[u16]) {
    unsafe {
        let pat = wz(&wcat(&[dir, w!("\\*")]));
        let mut fd: WIN32_FIND_DATAW = core::mem::zeroed();
        let h = FindFirstFileW(pat.as_ptr(), &mut fd);
        if h != INVALID_HANDLE_VALUE {
            loop {
                let name = trim_nul(&fd.cFileName).to_vec();
                if name != [46u16] && name != [46u16, 46] {
                    let p = wcat(&[dir, w!("\\"), &name]);
                    if fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                        remove_tree(&p);
                    } else {
                        SetFileAttributesW(wz(&p).as_ptr(), 0x80);
                        DeleteFileW(wz(&p).as_ptr());
                    }
                }
                if FindNextFileW(h, &mut fd) == 0 {
                    break;
                }
            }
            FindClose(h);
        }
        RemoveDirectoryW(wz(dir).as_ptr());
    }
}

fn do_uninstall(restore: bool) {
    quit_running();
    let exe = store::own_exe();
    store::set_autostart_for(&exe, false);
    store::set_desktop_menu_for(&exe, false);
    unsafe { DeleteFileW(wz(&start_menu_link()).as_ptr()) };
    store::reg_delete_tree(UNINSTALL_KEY);
    store::reg_delete_tree(crate::update::KEY);
    if restore {
        // every shortcut goes back to the desktop, then Crow's data is removed
        let root = store::root();
        let pat = wz(&wcat(&[&root, w!("\\f*")]));
        unsafe {
            let mut fd: WIN32_FIND_DATAW = core::mem::zeroed();
            let h = FindFirstFileW(pat.as_ptr(), &mut fd);
            if h != INVALID_HANDLE_VALUE {
                loop {
                    if fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                        let d = wcat(&[&root, w!("\\"), trim_nul(&fd.cFileName)]);
                        for f in list_files(&d) {
                            store::export_to_desktop(&wcat(&[&d, w!("\\"), &f]));
                        }
                    }
                    if FindNextFileW(h, &mut fd) == 0 {
                        break;
                    }
                }
                FindClose(h);
            }
        }
        remove_tree(&root);
    }
    remove_tree(&wcat(&[&env(w!("LOCALAPPDATA")), w!("\\Crow FO")]));
    // A running program can't delete its own file, but it can move it. It goes
    // to the temp folder, which Windows cleans up, and the folder is removed.
    let dir = parent_dir(&exe).to_vec();
    if weq_ic(&dir, &install_dir()) {
        let mut tmp = [0u16; 300];
        let n = unsafe { GetTempPathW(300, tmp.as_mut_ptr()) } as usize;
        let mut name = String::from("crow-removed-");
        fmt_int(unsafe { GetTickCount() } as i64, &mut name);
        name.push_str(".exe");
        let dst = wcat(&[&tmp[..n], &to_w(&name)]);
        unsafe { MoveFileExW(wz(&exe).as_ptr(), wz(&dst).as_ptr(), MOVEFILE_REPLACE_EXISTING) };
        remove_tree(&dir);
    }
}

// ------------------------------------------------------------------ window
unsafe extern "system" fn dlg_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if DLG.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    let d = &mut *DLG;
    let m_px = (MARGIN * d.sc) as i32;
    let pt = |l: LPARAM| {
        let q = crate::app::lp_pt(l);
        ((q.x - m_px) as f32, (q.y - m_px) as f32)
    };
    match m {
        WM_NCHITTEST => {
            let q = crate::app::lp_pt(l);
            let (x, y) = ((q.x - d.x) as f32, (q.y - d.y) as f32);
            let bar = BAR * d.sc;
            if y >= 0.0 && y < bar && x >= 0.0 && x < d.w as f32 - bar {
                return HTCAPTION;
            }
            return HTCLIENT;
        }
        WM_MOVE => {
            let mut r = RECT::default();
            GetWindowRect(h, &mut r);
            d.x = r.left + m_px;
            d.y = r.top + m_px;
        }
        WM_MOUSEMOVE => {
            let (x, y) = pt(l);
            if !d.inp.inside {
                let mut t = TRACKMOUSEEVENT { cbSize: core::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                TrackMouseEvent(&mut t);
            }
            d.inp.mx = x;
            d.inp.my = y;
            d.inp.inside = true;
            frame(d, true);
        }
        WM_MOUSELEAVE => {
            d.inp.inside = false;
            frame(d, true);
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let (x, y) = pt(l);
            d.inp.mx = x;
            d.inp.my = y;
            d.inp.down = true;
            d.inp.pressed = true;
            SetCapture(h);
            frame(d, false);
            frame(d, true);
        }
        WM_LBUTTONUP => {
            let (x, y) = pt(l);
            d.inp.mx = x;
            d.inp.my = y;
            d.inp.down = false;
            d.inp.released = true;
            ReleaseCapture();
            frame(d, false);
            frame(d, true);
        }
        WM_UPDATE_DONE => {
            KillTimer(h, 2);
            if w == 1 {
                // the installer is running and needs this file closed
                d.quit = true;
            } else {
                d.step = Step::Failed;
                frame(d, true);
            }
        }
        WM_TIMER if w == 2 => frame(d, true),
        WM_TIMER => {
            if !d.anim.step(16.0) {
                KillTimer(h, 1);
            }
            frame(d, true);
        }
        WM_KEYDOWN => {
            if w == VK_ESCAPE {
                d.quit = true;
            }
        }
        WM_SETCURSOR => {
            if (l & 0xFFFF) as isize == HTCAPTION {
                return DefWindowProcW(h, m, w, l);
            }
            SetCursor(LoadCursorW(0, d.cursor));
            return 1;
        }
        WM_CLOSE => d.quit = true,
        _ => return DefWindowProcW(h, m, w, l),
    }
    0
}

/// One UI pass. With `draw`, renders and shows the window.
unsafe fn frame(d: &mut Dlg, draw: bool) {
    let sc = d.sc;
    let m = MARGIN * sc;
    let (cw, ch) = (d.w + 2 * m as i32, d.h + 2 * m as i32);
    if draw && !matches!(&d.cv, Some(c) if c.w == cw && c.h == ch) {
        d.cv = Canvas::dib(cw, ch);
    }
    let th = Theme::dark(0xFFF5B841);
    let mut cvo = if draw { d.cv.take() } else { None };
    if let Some(cv) = cvo.as_mut() {
        cv.clear(0);
        let (w, h) = (d.w as f32, d.h as f32);
        cv.shadow(m, m + 6.0 * sc, w, h, 14.0 * sc, 0x66000000, 255, 18.0 * sc);
        cv.rrect(m, m, w, h, 14.0 * sc, th.bg, th.bg, 255, 1.0);
        cv.rrect_stroke(m, m, w, h, 14.0 * sc, 1.0 * sc, th.border, th.border, 255);
    }
    let mut inp = d.inp;
    inp.mx += m;
    inp.my += m;
    let face = String::from("Segoe UI Variable Text");
    let (pw, ph) = (d.w as f32, d.h as f32);
    let mut action = 0u8; // 1 primary, 2 secondary
    {
        let mut ui = Ui {
            cv: cvo.as_mut(),
            sc,
            x: m + 28.0 * sc,
            y: m,
            w: pw - 56.0 * sc,
            inp,
            active: &mut d.active,
            anim: &mut d.anim,
            texts: &mut d.texts,
            icons: &mut d.icons,
            fonts: &mut d.fonts,
            face: &face,
            th,
            cursor: IDC_ARROW,
            view: (m, m + ph),
            released_id: 0,
        };
        let cs = 32.0 * sc;
        if ui.icon_button(1, Ic::Close, m + pw - cs - 8.0 * sc, m + 6.0 * sc, cs) {
            action = 9;
        }
        // logo, name and what this window is about
        ui.y = m + BAR * sc;
        if let Some(cv) = ui.cv.as_deref_mut() {
            let gs = (56.0 * sc) as i32;
            let px = crate::art::logo(gs);
            cv.blit(&px, gs, gs, ui.x, ui.y, gs as f32, gs as f32, 256);
        }
        let (tx, ty) = (ui.x + 72.0 * sc, ui.y + 4.0 * sc);
        let (tc, mc) = (th.text, th.muted);
        let title = if d.uninstall { tr("uninstall.title") } else { "Crow FO" };
        ui.text(title, tx, ty, 22.0, FW_SEMIBOLD, tc, false);
        let mut sub = String::from(if d.uninstall {
            ""
        } else if d.update.is_some() {
            tr("update.subtitle")
        } else {
            tr("setup.subtitle")
        });
        if !d.uninstall && d.update.is_none() {
            sub.push_str("  ·  ");
            sub.push_str(env!("CROW_VERSION"));
        }
        ui.text(&sub, tx, ty + 32.0 * sc, 12.5, FW_NORMAL, mc, false);
        ui.y += 80.0 * sc;

        let full = ui.w;
        let half = (full - 8.0 * sc) / 2.0;
        let x0 = ui.x;
        if let Some(tag) = d.update.as_deref() {
            match d.step {
                Step::Ask => {
                    let ver = tag.trim_start_matches('v');
                    let text = tr("update.text").replacen("{}", ver, 1).replacen("{}", env!("CARGO_PKG_VERSION"), 1);
                    ui.paragraph(&text, 13.0, tc);
                    ui.y = m + ph - 58.0 * sc;
                    if ui.button(30, tr("update.later"), None, 0, x0, half) {
                        action = 9;
                    }
                    if ui.button(31, tr("update.now"), None, 1, x0 + half + 8.0 * sc, half) {
                        action = 3;
                    }
                }
                Step::Working | Step::Done => {
                    ui.text(tr("update.downloading"), x0, ui.y, 13.0, FW_NORMAL, tc, false);
                    let k = crate::net::PROGRESS.load(core::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
                    let (by, bh) = (ui.y + 30.0 * sc, 6.0 * sc);
                    if let Some(cv) = ui.cv.as_deref_mut() {
                        cv.rrect(x0, by, full, bh, bh / 2.0, th.track, th.track, 255, 1.0);
                        if k > 0.0 {
                            cv.rrect(x0, by, (full * k).max(bh), bh, bh / 2.0, th.accent, th.accent, 255, 1.0);
                        }
                    }
                }
                Step::Failed => {
                    ui.paragraph(tr("update.failed"), 13.0, th.danger);
                    ui.y = m + ph - 58.0 * sc;
                    if ui.button(32, tr("setup.close"), None, 0, x0, half) {
                        action = 9;
                    }
                    if ui.button(33, tr("prefs.project_page"), None, 1, x0 + half + 8.0 * sc, half) {
                        action = 4;
                    }
                }
            }
        } else {
        match (d.uninstall, d.step) {
            (false, Step::Ask) => {
                ui.toggle(10, tr("setup.startup"), &mut d.startup);
                ui.toggle(11, tr("setup.desktop_menu"), &mut d.menu);
                ui.toggle(13, tr("setup.check_updates"), &mut d.updates);
                ui.space(6.0);
                ui.paragraph(tr("setup.where"), 12.0, mc);
                ui.y = m + ph - 58.0 * sc;
                let label = if d.updating { tr("setup.update") } else { tr("setup.install") };
                if ui.button(20, label, None, 1, x0, full) {
                    action = 1;
                }
            }
            (false, Step::Done) => {
                ui.text(tr("setup.done"), x0, ui.y, 16.0, FW_SEMIBOLD, tc, false);
                ui.y = m + ph - 58.0 * sc;
                if ui.button(21, tr("setup.close"), None, 0, x0, half) {
                    action = 9;
                }
                if ui.button(22, tr("setup.open"), None, 1, x0 + half + 8.0 * sc, half) {
                    action = 2;
                }
            }
            (true, Step::Ask) => {
                ui.paragraph(tr("uninstall.text"), 13.0, tc);
                ui.toggle(12, tr("uninstall.restore"), &mut d.restore);
                ui.y = m + ph - 58.0 * sc;
                if ui.button(23, tr("uninstall.cancel"), None, 0, x0, half) {
                    action = 9;
                }
                if ui.button(24, tr("uninstall.button"), None, 3, x0 + half + 8.0 * sc, half) {
                    action = 1;
                }
            }
            (true, _) => {
                ui.text(tr("uninstall.done"), x0, ui.y, 16.0, FW_SEMIBOLD, tc, false);
                ui.y = m + ph - 58.0 * sc;
                if ui.button(25, tr("setup.close"), None, 0, x0, full) {
                    action = 9;
                }
            }
            (false, Step::Failed) => {
                ui.paragraph(tr("setup.failed"), 13.0, th.danger);
                ui.y = m + ph - 58.0 * sc;
                if ui.button(26, tr("setup.close"), None, 0, x0, full) {
                    action = 9;
                }
            }
            (false, Step::Working) => {}
        }
        }
        d.cursor = ui.cursor;
    }
    if d.inp.released {
        d.active = 0;
    }
    d.inp.pressed = false;
    d.inp.released = false;
    if draw {
        d.cv = cvo;
        if let Some(cv) = d.cv.as_ref() {
            let pos = POINT { x: d.x - m as i32, y: d.y - m as i32 };
            let size = SIZE { cx: cv.w, cy: cv.h };
            let src = POINT::default();
            let bf = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA };
            UpdateLayeredWindow(d.hwnd, 0, &pos, &size, cv.dc, &src, 0, &bf, ULW_ALPHA);
        }
        if d.anim.busy() {
            SetTimer(d.hwnd, 1, 16, 0);
        }
    }
    match action {
        1 if !d.uninstall => {
            d.step = if install(d.payload, d.startup, d.menu, Some(d.updates)) { Step::Done } else { Step::Failed };
            frame(d, true);
        }
        1 => {
            do_uninstall(d.restore);
            d.step = Step::Done;
            frame(d, true);
        }
        2 => {
            let exe = wz(&wcat(&[&install_dir(), w!("\\CrowFO.exe")]));
            ShellExecuteW(0, core::ptr::null(), exe.as_ptr(), core::ptr::null(), core::ptr::null(), SW_SHOWNORMAL);
            d.quit = true;
        }
        3 => {
            d.step = Step::Working;
            crate::net::PROGRESS.store(0, core::sync::atomic::Ordering::Relaxed);
            SetTimer(d.hwnd, 2, 100, 0);
            CreateThread(core::ptr::null(), 0, download_thread, core::ptr::null_mut(), 0, core::ptr::null_mut());
            frame(d, true);
        }
        4 => {
            let url = wz(&to_w(&crate::update::releases_page()));
            ShellExecuteW(0, core::ptr::null(), url.as_ptr(), core::ptr::null(), core::ptr::null(), SW_SHOWNORMAL);
            d.quit = true;
        }
        9 => d.quit = true,
        _ => {}
    }
}

unsafe extern "system" fn download_thread(_: *mut core::ffi::c_void) -> u32 {
    let d = &*DLG;
    let ok = d.update.as_deref().is_some_and(crate::update::download_and_run);
    PostMessageW(d.hwnd, WM_UPDATE_DONE, ok as usize, 0);
    0
}

fn run(uninstall: bool, payload: &'static [u8], update: Option<String>) {
    unsafe {
        let inst = GetModuleHandleW(core::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: dlg_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: LoadIconW(inst, 1),
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: 0,
            lpszMenuName: core::ptr::null(),
            lpszClassName: w!("CrowDialog").as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);
        let offer = update.is_some();
        crate::i18n::set(&if offer { crate::update::language() } else { "auto".into() });
        let sc = GetDpiForSystem().max(96) as f32 / 96.0;
        let h = if uninstall { 330.0 } else if offer { 270.0 } else { 396.0 };
        let (w, h) = ((440.0 * sc) as i32, (h * sc) as i32);
        let wa = crate::popup::work_area(crate::app::cursor());
        // an update offer sits in the corner like a notification
        let (x, y) = if offer {
            (wa.right - w - (12.0 * sc) as i32, wa.bottom - h - (12.0 * sc) as i32)
        } else {
            (wa.left + (wa.right - wa.left - w) / 2, wa.top + (wa.bottom - wa.top - h) / 2)
        };
        let updating = installed_version().is_some();
        // the update check starts on, unless the user turned it off before
        let updates = crate::update::choice().unwrap_or(true);
        let d = alloc::boxed::Box::leak(alloc::boxed::Box::new(Dlg {
            hwnd: 0,
            uninstall,
            update,
            payload,
            sc,
            x,
            y,
            w,
            h,
            cv: None,
            texts: Texts::new(),
            icons: Icons::new(),
            anim: Anim::new(),
            fonts: Fonts::new(),
            inp: Input::default(),
            active: 0,
            cursor: IDC_ARROW,
            step: Step::Ask,
            startup: true,
            menu: true,
            updates,
            restore: true,
            updating,
            quit: false,
        }));
        DLG = d;
        let m = (MARGIN * sc) as i32;
        let title = wz(&to_w(if uninstall { tr("uninstall.title") } else { "Crow FO" }));
        let ex = if offer { WS_EX_TOPMOST } else { 0 };
        d.hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_APPWINDOW | ex, w!("CrowDialog").as_ptr(), title.as_ptr(), WS_POPUP,
            x - m, y - m, w + 2 * m, h + 2 * m, 0, 0, inst, core::ptr::null());
        frame(d, true);
        if offer {
            // don't take the keyboard away from whatever the user is doing
            ShowWindow(d.hwnd, SW_SHOWNOACTIVATE);
        } else {
            ShowWindow(d.hwnd, SW_SHOW);
            SetForegroundWindow(d.hwnd);
        }
        let mut msg: MSG = core::mem::zeroed();
        while !d.quit && GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DestroyWindow(d.hwnd);
    }
}

/// Whether a key (and optionally one of its values) exists under HKCU.
fn reg_has(path: &[u16], value: Option<&[u16]>) -> bool {
    unsafe {
        let mut k: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, wz(path).as_ptr(), 0, KEY_READ, &mut k) != 0 {
            return false;
        }
        let ok = match value {
            None => true,
            Some(v) => {
                let n = wz(v);
                let null = core::ptr::null_mut();
                RegQueryValueExW(k, n.as_ptr(), null, null, null as *mut u8, null) == 0
            }
        };
        RegCloseKey(k);
        ok
    }
}

/// Entry point of CrowFOSetup.exe. `/S` installs without any window, for winget
/// and scripts. An update keeps the current startup, desktop menu and update
/// check choices, a new install turns them all on.
pub fn setup(payload: &'static [u8]) {
    let cl = unsafe { from_w(core::slice::from_raw_parts(GetCommandLineW(), crate::rt::wcslen(GetCommandLineW()).min(32768))) };
    let silent = cl.split_whitespace().skip(1).any(|a| a.eq_ignore_ascii_case("/S") || a.eq_ignore_ascii_case("--silent"));
    if silent {
        let fresh = installed_version().is_none();
        let startup = fresh || reg_has(store::RUN_KEY, Some(w!("Crow FO")));
        let menu = fresh || reg_has(store::DESK_KEY, None);
        let ok = install(payload, startup, menu, fresh.then_some(true));
        if ok {
            let exe = wz(&wcat(&[&install_dir(), w!("\\CrowFO.exe")]));
            unsafe { ShellExecuteW(0, core::ptr::null(), exe.as_ptr(), w!("--background").as_ptr(), core::ptr::null(), SW_SHOWNORMAL) };
        }
        unsafe { ExitProcess(if ok { 0 } else { 1 }) };
    }
    run(false, payload, None);
}

/// `CrowFO.exe --uninstall`, started from "Installed apps".
pub fn uninstall() {
    run(true, &[], None);
}

/// `CrowFO.exe --check-update` found release `tag`: offer it.
pub fn offer_update(tag: String) {
    run(false, &[], Some(tag));
}
