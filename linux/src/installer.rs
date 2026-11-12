//! Installer and uninstaller for Linux, with the same window and texts as on
//! Windows. Everything is per user, no root needed:
//!
//!   ~/.local/bin/crowfo                              the app
//!   ~/.local/share/applications/crowfo.desktop      the menu entry (with "Uninstall")
//!   ~/.local/share/icons/hicolor/*/apps/crowfo*.png  icons
//!   ~/.config/autostart/crowfo.desktop              start at login (optional)
//!   file manager menu entries                       see store::set_desktop_menu
//!
//! Running the downloaded file opens this window when Crow FO isn't installed
//! yet. `--install --quiet` installs without it (used by updates and scripts).

use crate::app::Fonts;
use crate::ffi::*;
use crate::gfx::*;
use crate::i18n::tr;
use crate::store;
use crate::uikit::*;
use crate::util::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

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

/// 0 while the download runs, then 1 when it worked and 2 when it didn't.
static DOWNLOAD: AtomicU8 = AtomicU8::new(0);

struct Dlg {
    hwnd: HWND,
    uninstall: bool,
    /// The tag of the new release when this window offers an update.
    update: Option<String>,
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

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

pub fn exe_path() -> PathBuf {
    home().join(".local/bin/crowfo")
}

pub fn install_dir() -> WString {
    to_w(&home().join(".local/bin").to_string_lossy())
}

fn menu_entry() -> PathBuf {
    store::data_home().join("applications/crowfo.desktop")
}

fn icon_files() -> Vec<(PathBuf, i32, &'static str)> {
    let base = store::data_home().join("icons/hicolor");
    let mut v = Vec::new();
    for s in [16, 24, 32, 48, 64, 128, 256] {
        let d = base.join(format!("{s}x{s}/apps"));
        v.push((d.join("crowfo.png"), s, ""));
        if s <= 32 {
            v.push((d.join("crowfo-new.png"), s, crate::art::MENU_NEW));
            v.push((d.join("crowfo-panel.png"), s, crate::art::MENU_PANEL));
            v.push((d.join("crowfo-settings.png"), s, crate::art::MENU_SETTINGS));
        }
    }
    v
}

/// Puts the app and menu icons in the user's icon theme.
pub fn install_icons() {
    for (path, s, body) in icon_files() {
        if path.exists() {
            continue;
        }
        let px = if body.is_empty() { crate::art::logo(s) } else { crate::art::menu_icon(body, s) };
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, crate::art::png(&px, s));
    }
}

fn installed() -> bool {
    exe_path().is_file() && menu_entry().is_file()
}

/// Asks a running Crow to save and quit, and waits for it.
fn quit_running() {
    if !crate::instance::send("quit") {
        return;
    }
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !crate::instance::is_running() {
            return;
        }
    }
}

/// `updates` is None to keep the current update check setting.
fn install(startup: bool, menu: bool, updates: Option<bool>) -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let dst = exe_path();
    let _ = std::fs::create_dir_all(dst.parent().unwrap());
    if std::fs::canonicalize(&me).ok() != std::fs::canonicalize(&dst).ok() {
        // A new file renamed over the old one: a running copy keeps its own.
        let tmp = dst.with_extension("new");
        if std::fs::copy(&me, &tmp).is_err() {
            return false;
        }
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        if std::fs::rename(&tmp, &dst).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
    }
    install_icons();
    let exe = store::exec_quote(&dst.to_string_lossy());
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Crow FO\nGenericName={}\nComment={}\nExec={exe} --settings\nIcon=crowfo\nTerminal=false\n\
         Categories=Utility;\nKeywords=folder;organizer;desktop;shortcuts;\nStartupNotify=false\nX-Crow-Version={}\nActions=uninstall;\n\n\
         [Desktop Action uninstall]\nName={}\nExec={exe} --uninstall\n",
        tr("setup.subtitle"),
        tr("setup.subtitle"),
        env!("CROW_VERSION"),
        tr("uninstall.title"),
    );
    let me_entry = menu_entry();
    let _ = std::fs::create_dir_all(me_entry.parent().unwrap());
    if std::fs::write(&me_entry, entry).is_err() {
        return false;
    }
    let dst_w = to_w(&dst.to_string_lossy());
    store::set_autostart_for(&dst_w, startup);
    store::set_desktop_menu_for(&dst_w, menu);
    if let Some(on) = updates {
        crate::update::set_enabled(on);
    }
    refresh_menus();
    true
}

/// Tells desktops to read menu entries and icons again.
fn refresh_menus() {
    for (cmd, args) in [
        ("update-desktop-database", vec![store::data_home().join("applications").to_string_lossy().into_owned()]),
        ("gtk-update-icon-cache", vec!["-f".into(), "-t".into(), store::data_home().join("icons/hicolor").to_string_lossy().into_owned()]),
    ] {
        if has_program(cmd) {
            let _ = std::process::Command::new(cmd).args(&args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
    }
}

fn do_uninstall(restore: bool) {
    quit_running();
    let exe = store::own_exe();
    store::set_autostart_for(&exe, false);
    store::set_desktop_menu_for(&exe, false);
    let _ = std::fs::remove_file(menu_entry());
    for (p, _, _) in icon_files() {
        let _ = std::fs::remove_file(p);
    }
    let local = PathBuf::from(from_w(&store::cache_dir()));
    let _ = std::fs::remove_dir_all(&local);
    if let Some(cfg) = crate::update::settings_dir() {
        let _ = std::fs::remove_dir_all(cfg);
    }
    if restore {
        // Everything in the folders goes back to the desktop, then Crow's own
        // data is removed. Only empty folders are deleted, so anything that
        // couldn't be moved stays safe where it is.
        let root = PathBuf::from(from_w(&store::root()));
        let mut all_out = true;
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with('f') || !e.path().is_dir() {
                    continue;
                }
                let d = to_w(&e.path().to_string_lossy());
                for f in list_files(&d) {
                    store::export_to_desktop(&wcat(&[&d, w!("/"), &f]));
                }
                all_out &= std::fs::remove_dir(e.path()).is_ok();
            }
        }
        if all_out {
            let _ = std::fs::remove_file(root.join("crow.ini"));
            let _ = std::fs::remove_dir(&root);
        }
    }
    refresh_menus();
    // Linux lets a running program delete its own file.
    let me = std::env::current_exe().ok().and_then(|p| std::fs::canonicalize(p).ok());
    if me.is_some() && me == std::fs::canonicalize(exe_path()).ok() {
        let _ = std::fs::remove_file(exe_path());
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
        WM_TIMER if w == 2 => {
            match DOWNLOAD.load(Ordering::Relaxed) {
                // the new version installs itself and starts again
                1 => {
                    KillTimer(h, 2);
                    d.quit = true;
                }
                2 => {
                    KillTimer(h, 2);
                    d.step = Step::Failed;
                }
                _ => {}
            }
            frame(d, true);
        }
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
                    let k = crate::net::PROGRESS.load(Ordering::Relaxed) as f32 / 1000.0;
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
            d.step = if install(d.startup, d.menu, Some(d.updates)) { Step::Done } else { Step::Failed };
            frame(d, true);
        }
        1 => {
            do_uninstall(d.restore);
            d.step = Step::Done;
            frame(d, true);
        }
        2 => {
            // the window closes first, so the app's own window comes to the front
            quit_running();
            spawn(&exe_path().to_string_lossy(), &["--settings"], None);
            d.quit = true;
        }
        3 => {
            d.step = Step::Working;
            crate::net::PROGRESS.store(0, Ordering::Relaxed);
            DOWNLOAD.store(0, Ordering::Relaxed);
            SetTimer(d.hwnd, 2, 100, 0);
            let tag = d.update.clone().unwrap_or_default();
            std::thread::spawn(move || {
                let ok = crate::update::download_and_run(&tag);
                DOWNLOAD.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            });
            frame(d, true);
        }
        4 => {
            open_default(&crate::update::releases_page());
            d.quit = true;
        }
        9 => d.quit = true,
        _ => {}
    }
}

fn run(uninstall: bool, update: Option<String>) {
    unsafe {
        let inst = GetModuleHandleW(core::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: dlg_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: 0,
            lpszMenuName: core::ptr::null(),
            lpszClassName: w!("CrowDialog").as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);
        let offer = update.is_some();
        crate::i18n::set(&crate::update::language());
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
        let updating = installed();
        // the update check starts on, unless the user turned it off before
        let updates = crate::update::choice().unwrap_or(true);
        let fresh = !updating;
        let d = Box::leak(Box::new(Dlg {
            hwnd: 0,
            uninstall,
            update,
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
            startup: fresh || store::autostart_file().exists(),
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
        ShowWindow(d.hwnd, SW_SHOW);
        if !offer {
            // an update offer doesn't take the keyboard away from what the user is doing
            SetForegroundWindow(d.hwnd);
        }
        let xfd = crate::ffi::x::x().fd();
        while !d.quit {
            if !pump() {
                break;
            }
            if d.quit {
                break;
            }
            if has_pending() || crate::ffi::x::x().buffered() {
                continue;
            }
            let t = next_timer_ms().map_or(-1, |v| v.min(1000) as i32);
            let mut p = libc::pollfd { fd: xfd, events: libc::POLLIN, revents: 0 };
            libc::poll(&mut p, 1, t);
        }
        DestroyWindow(d.hwnd);
        crate::ffi::x::x().flush();
    }
}

/// `crowfo --install`: the setup window, or with --quiet a silent install
/// that keeps the current choices (a new install turns them all on).
pub fn install_cli(quiet: bool) -> i32 {
    if !quiet {
        run(false, None);
        return 0;
    }
    let fresh = !installed();
    let was_running = crate::instance::is_running();
    let startup = fresh || store::autostart_file().exists();
    let menu = fresh || store::desktop_menu_present();
    crate::i18n::set(&crate::update::language());
    quit_running();
    let ok = install(startup, menu, fresh.then_some(true));
    if ok && (was_running || fresh) {
        spawn(&exe_path().to_string_lossy(), &["--background"], None);
    }
    if ok { 0 } else { 1 }
}

/// Whether the first start should offer to install: a downloaded copy run
/// directly, with nothing installed yet.
pub fn should_offer_install() -> bool {
    !installed() && std::env::var_os("CROW_PORTABLE").is_none()
}

/// `crowfo --uninstall`, started from the menu entry.
pub fn uninstall() {
    run(true, None);
}

/// `crowfo --check-update` found release `tag`: offer it.
pub fn offer_update(tag: String) {
    run(false, Some(tag));
}
