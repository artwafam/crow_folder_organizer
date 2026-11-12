//! Application state, window setup, the animation loop and saving settings.

use crate::config::{Config, Folder, Global, Mode};
use crate::desk;
use crate::ffi::*;
use crate::folder::FolderWin;
use crate::gfx::Canvas;
use crate::input::{PendingDrop, T_GHOST, WM_CROW_DROP};
use crate::popup::Popup;
use crate::store;
use crate::util::*;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

pub const T_HOVER: usize = 1;
pub const T_CLOSE: usize = 2;
pub const T_SPRING: usize = 3;
pub const T_IDLE: usize = 10;
/// Slow check that trims memory again when Windows threads inside the process
/// (shell extensions, COM) grew it while Crow itself stayed asleep.
pub const T_WATCH: usize = 12;
/// Sent by a second `CrowFO.exe --new` or `--new-panel`. wParam is 1 for a hover folder and 2 for a panel.
pub const WM_CROW_NEW: u32 = WM_USER + 0x43;
/// Asks the running instance to show its settings window.
pub const WM_CROW_PREFS: u32 = WM_USER + 0x45;
/// Asks the running instance to save and quit (used by setup and uninstall).
pub const WM_CROW_QUIT: u32 = WM_USER + 0x46;
/// Sent by `CrowFO.exe --check-update --now`. wParam is one of the update::* states.
pub const WM_CROW_UPDATE: u32 = WM_USER + 0x47;

pub struct Fonts {
    v: Vec<(String, i32, i32, HFONT)>,
}
impl Fonts {
    pub const fn new() -> Fonts {
        Fonts { v: Vec::new() }
    }
    pub fn get(&mut self, face: &str, px: i32, weight: i32) -> HFONT {
        if let Some(f) = self.v.iter().find(|f| f.1 == px && f.2 == weight && f.0 == face) {
            return f.3;
        }
        let h = crate::text::font(face, px, weight);
        self.v.push((face.into(), px, weight, h));
        h
    }
    pub fn clear(&mut self) {
        for f in self.v.drain(..) {
            unsafe { DeleteObject(f.3) };
        }
    }
}
impl Drop for Fonts {
    fn drop(&mut self) {
        self.clear();
    }
}

pub struct App {
    pub g: Global,
    pub wins: Vec<Box<FolderWin>>,
    pub fonts: Fonts,
    pub msg: HWND,
    pub ini: WString,
    pub last_ini: Vec<u8>,
    pub topmost: bool,
    pub ghost: HWND,
    pub ghost_cv: Option<Canvas>,
    /// Where the ghost sits relative to the pointer, and how see-through it is.
    pub ghost_off: POINT,
    pub ghost_alpha: u8,
    /// How the last "Check for updates" from the settings went (update::CHECKING...).
    pub update_state: u8,
    pub drag_src: Option<(u32, usize)>,
    pub last_frame: f64,
    pub was_animating: bool,
    pub ole_files: Vec<WString>,
    pub spring: u32,
    pub taskbar_msg: u32,
    pub pc: String,
    pub popup: Option<Box<Popup>>,
    pub prefs: Option<Box<Popup>>,
    pub pending_drop: Option<PendingDrop>,
    /// Set right after the working set was trimmed, so the timer that did it
    /// doesn't schedule yet another trim.
    pub trimmed: bool,
}

static mut APP: *mut App = core::ptr::null_mut();

#[inline(always)]
pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

pub fn lp_pt(l: LPARAM) -> POINT {
    POINT { x: (l & 0xFFFF) as i16 as i32, y: ((l >> 16) & 0xFFFF) as i16 as i32 }
}
pub fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) };
    p
}
pub fn in_rect(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

unsafe extern "system" fn folder_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(h, m, w, l);
    }
    let a = app();
    let Some(i) = a.by_hwnd(h) else { return DefWindowProcW(h, m, w, l) };
    match m {
        WM_MOUSEACTIVATE => return if a.wins[i].edit.is_some() { MA_ACTIVATE } else { MA_NOACTIVATE },
        WM_KEYDOWN | WM_SYSKEYDOWN if a.wins[i].edit.is_some() => a.edit_key(i, w),
        WM_CHAR if a.wins[i].edit.is_some() => a.edit_char(i, w as u16),
        WM_KILLFOCUS if a.wins[i].edit.is_some() => a.commit_edit(i, true),
        WM_MOUSEMOVE => a.on_move(i, lp_pt(l)),
        WM_MOUSELEAVE => a.on_leave(i),
        WM_LBUTTONDOWN => a.on_down(i, lp_pt(l)),
        WM_LBUTTONUP => a.on_up(i, lp_pt(l)),
        WM_LBUTTONDBLCLK => a.on_dbl(i, lp_pt(l)),
        WM_RBUTTONUP => a.on_right(i, lp_pt(l)),
        WM_MOUSEWHEEL => a.on_wheel(i, ((w >> 16) & 0xFFFF) as i16 as i32),
        WM_TIMER => a.on_timer(i, w),
        WM_CAPTURECHANGED => a.on_capture_lost(i),
        WM_SETCURSOR => {
            if a.on_cursor(i) {
                return 1;
            }
            return DefWindowProcW(h, m, w, l);
        }
        WM_DPICHANGED => a.on_dpi(i, (w & 0xFFFF) as u32),
        _ => return DefWindowProcW(h, m, w, l),
    }
    0
}

unsafe extern "system" fn host_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if !APP.is_null() {
        let a = app();
        if m == WM_TIMER && w == T_IDLE {
            a.on_idle();
            return 0;
        }
        if m == WM_TIMER && w == T_WATCH {
            a.on_watch();
            return 0;
        }
        if m == WM_TIMER && w == T_GHOST {
            a.check_ghost();
            return 0;
        }
        if m == WM_CROW_DROP {
            a.process_drop();
            return 0;
        }
        if m == WM_CROW_PREFS {
            a.open_prefs();
            return 0;
        }
        if m == WM_CROW_UPDATE && (w as u8) >= crate::update::LATEST && w <= crate::update::FOUND as usize {
            a.update_state = w as u8;
            a.pass(crate::popup::Slot::Prefs, true);
            return 0;
        }
        if m == WM_CROW_QUIT {
            a.save();
            PostQuitMessage(0);
            return 0;
        }
        if m == WM_CROW_NEW && (w == 1 || w == 2) {
            a.new_folder_at_cursor(w == 2);
            return 0;
        }
        if (m == a.taskbar_msg && m != 0) || m == WM_DISPLAYCHANGE || (m == WM_SETTINGCHANGE && w == 0x2F) {
            a.relayout_all();
            return 0;
        }
    }
    DefWindowProcW(h, m, w, l)
}

unsafe extern "system" fn fg_hook(_: HWINEVENTHOOK, _ev: u32, h: HWND, _o: i32, _c: i32, _t: u32, _ms: u32) {
    if !APP.is_null() {
        app().on_foreground(h);
    }
}

impl App {
    pub fn init() -> &'static mut App {
        let pc = store::computer_name();
        let ini = store::ini_path();
        let data = read_file_max(&ini, 2 << 20);
        let mut cfg = match &data {
            Some(d) => Config::parse(core::str::from_utf8(d).unwrap_or("")),
            None => default_config(),
        };
        for f in cfg.folders.iter_mut() {
            use_pc_pos(f, &pc);
        }
        crate::i18n::set(&cfg.g.language);
        let a = Box::new(App {
            g: cfg.g,
            wins: Vec::new(),
            fonts: Fonts::new(),
            msg: 0,
            ini,
            last_ini: data.unwrap_or_default(),
            topmost: false,
            ghost: 0,
            ghost_cv: None,
            ghost_off: POINT { x: 0, y: 0 },
            ghost_alpha: 255,
            update_state: 0,
            drag_src: None,
            last_frame: 0.0,
            was_animating: false,
            ole_files: Vec::new(),
            spring: 0,
            taskbar_msg: 0,
            pc,
            popup: None,
            prefs: None,
            pending_drop: None,
            trimmed: false,
        });
        unsafe {
            APP = Box::into_raw(a);
            let inst = GetModuleHandleW(core::ptr::null());
            let mut wc = WNDCLASSEXW {
                cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: folder_proc,
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: inst,
                hIcon: 0,
                hCursor: LoadCursorW(0, IDC_ARROW),
                hbrBackground: 0,
                lpszMenuName: core::ptr::null(),
                lpszClassName: w!("CrowFolder").as_ptr(),
                hIconSm: 0,
            };
            RegisterClassExW(&wc);
            wc.style = 0;
            wc.lpfnWndProc = DefWindowProcW;
            wc.hbrBackground = GetStockObject(4); // a black brush is fully transparent under DWM blur
            wc.lpszClassName = w!("CrowBackdrop").as_ptr();
            RegisterClassExW(&wc);
            wc.hbrBackground = 0;
            wc.lpfnWndProc = host_proc;
            wc.lpszClassName = w!("CrowFOHost").as_ptr();
            RegisterClassExW(&wc);
            crate::popup::register_class();
            let a = app();
            a.msg = CreateWindowExW(WS_EX_TOOLWINDOW, w!("CrowFOHost").as_ptr(), w!("Crow FO").as_ptr(), WS_POPUP, 0, 0, 0, 0, 0, 0, inst, core::ptr::null());
            a.taskbar_msg = RegisterWindowMessageW(w!("TaskbarCreated").as_ptr());
            for f in cfg.folders {
                a.add_folder(f);
            }
            // Startup and the context menu are registered on the first run (and
            // by the installer). After that only the settings change them, so
            // the app never rewrites the registry on its own at each start.
            if data_missing(&a.last_ini) {
                a.save();
                store::set_autostart(a.g.autostart);
                store::set_desktop_menu(a.g.desktop_menu);
            }
            SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, 0, fg_hook, 0, 0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
            a.on_foreground(GetForegroundWindow());
            a.arm_idle();
            SetTimer(a.msg, T_WATCH, 20_000, 0);
            a
        }
    }

    pub fn by_hwnd(&self, h: HWND) -> Option<usize> {
        self.wins.iter().position(|w| w.hwnd == h)
    }
    pub fn by_id(&self, id: u32) -> Option<usize> {
        self.wins.iter().position(|w| w.cfg.id == id)
    }
    pub fn is_ours(&self, h: HWND) -> bool {
        h == self.ghost
            || self.popup.as_ref().is_some_and(|p| p.hwnd == h)
            || self.prefs.as_ref().is_some_and(|p| p.hwnd == h)
            || self.wins.iter().any(|w| w.hwnd == h || (w.backdrop == h && h != 0))
    }

    pub fn add_folder(&mut self, cfg: Folder) -> usize {
        let mut f = Box::new(FolderWin::new(cfg));
        unsafe {
            let inst = GetModuleHandleW(core::ptr::null());
            let name = wz(&to_w(&f.cfg.name));
            f.hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("CrowFolder").as_ptr(), name.as_ptr(),
                WS_POPUP, f.cfg.x, f.cfg.y, 1, 1, 0, 0, inst, core::ptr::null());
            f.sc = GetDpiForWindow(f.hwnd).max(96) as f32 / 96.0;
            f.place(&self.g);
            f.render(&self.g, &mut self.fonts);
            ShowWindow(f.hwnd, SW_SHOWNOACTIVATE);
            RegisterDragDrop(f.hwnd, crate::drop::target_ptr());
            let h = f.hwnd;
            self.wins.push(f);
            self.sink(h);
        }
        let k = self.wins.len() - 1;
        self.update_blur(k);
        self.wins.len() - 1
    }

    pub fn remove_folder(&mut self, i: usize) {
        if self.popup.as_ref().is_some_and(|p| p.folder() == self.wins[i].cfg.id) {
            self.close_popup(false);
        }
        let mut f = self.wins.remove(i);
        crate::blur::apply(&mut f, false, &self.g);
        unsafe {
            RevokeDragDrop(f.hwnd);
            DestroyWindow(f.hwnd);
        }
    }

    pub fn sink(&self, h: HWND) {
        if self.topmost || self.g.on_top {
            desk::set_topmost(h, true);
        } else {
            if unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0 {
                desk::set_topmost(h, false);
            }
            desk::sink(h, &|x| self.is_ours(x));
        }
        self.follow(h);
    }

    /// Re-stacks a folder's blur backdrop right below it after z-order changes.
    pub fn follow(&self, h: HWND) {
        if let Some(w) = self.wins.iter().find(|w| w.hwnd == h) {
            crate::blur::restack(w);
        }
    }

    pub fn on_foreground(&mut self, h: HWND) {
        if self.is_ours(h) || self.g.on_top {
            return;
        }
        let d = desk::is_desktop(h);
        if d == self.topmost {
            return;
        }
        self.topmost = d;
        for w in &self.wins {
            if d {
                desk::set_topmost(w.hwnd, true);
            } else {
                desk::set_topmost(w.hwnd, false);
                desk::sink(w.hwnd, &|x| self.is_ours(x));
            }
            crate::blur::restack(w);
        }
    }

    pub fn render(&mut self, i: usize) {
        let f = &mut self.wins[i];
        f.render(&self.g, &mut self.fonts);
        self.update_blur(i);
        // anything drawn means buffers are in use again, so free them later
        self.arm_idle();
    }

    /// Places and redraws every folder, used after config, DPI or monitor changes.
    pub fn relayout_all(&mut self) {
        for i in 0..self.wins.len() {
            let f = &mut self.wins[i];
            f.sc = unsafe { GetDpiForWindow(f.hwnd).max(96) } as f32 / 96.0;
            f.invalidate_content();
            f.place(&self.g);
            self.render(i);
            self.sink(self.wins[i].hwnd);
        }
    }

    pub fn update_blur(&mut self, i: usize) {
        let on = self.g.blur && self.wins[i].expanded() && self.wins[i].e() > 0.05;
        crate::blur::apply(&mut self.wins[i], on, &self.g);
    }

    pub fn any_animating(&self) -> bool {
        self.wins.iter().any(|w| w.animating())
            || self.popup.as_ref().is_some_and(|p| p.animating())
            || self.prefs.as_ref().is_some_and(|p| p.animating())
    }

    /// One animation frame for every animating folder.
    pub fn tick(&mut self) {
        let now = now_ms();
        let dt = if self.was_animating { clamp((now - self.last_frame) as f32, 1.0, 50.0) } else { 16.0 };
        self.last_frame = now;
        let mut any = false;
        for i in 0..self.wins.len() {
            if !self.wins[i].animating() {
                continue;
            }
            let still = self.wins[i].step(&self.g, dt);
            let f = &mut self.wins[i];
            let rolled_up = f.cfg.mode == Mode::Panel && f.cfg.collapsed && f.roll <= 0.0;
            if !still && rolled_up {
                // the panel finished rolling up, shrink its window to the header
                f.place(&self.g);
                self.render(i);
            } else if !still && f.cfg.mode == Mode::Hover && f.t == 0.0 && f.target == 0.0 {
                // Fully collapsed, so shrink the window back to the icon and send it down to the desktop layer
                f.scroll = 0.0;
                f.scroll_target = 0.0;
                f.place(&self.g);
                self.render(i);
                self.sink(self.wins[i].hwnd);
            } else {
                self.render(i);
            }
            any |= still;
        }
        if self.popup.is_some() || self.prefs.is_some() {
            any |= self.popup_step(dt);
        }
        self.was_animating = any;
        if !any {
            self.arm_idle();
        }
    }

    pub fn expand(&mut self, i: usize) {
        let f = &mut self.wins[i];
        if f.cfg.mode != Mode::Hover || f.target == 1.0 {
            return;
        }
        unsafe { KillTimer(f.hwnd, T_HOVER) };
        f.target = 1.0;
        f.place(&self.g);
        if !self.topmost && !self.g.on_top {
            desk::raise(f.hwnd);
        }
        self.render(i);
    }
    pub fn collapse(&mut self, i: usize) {
        let f = &mut self.wins[i];
        if f.cfg.mode != Mode::Hover || f.target == 0.0 {
            return;
        }
        f.target = 0.0;
        f.hover = -1;
        f.press = -1;
    }

    pub fn arm_idle(&self) {
        unsafe { SetTimer(self.msg, T_IDLE, 1500, 0) };
    }

    /// Called when nothing is happening. Frees every pixel buffer and hands the memory back to Windows.
    pub fn on_idle(&mut self) {
        unsafe { KillTimer(self.msg, T_IDLE) };
        if self.ghost != 0 && self.drag_src.is_none() {
            self.check_ghost();
        }
        if self.popup.is_none() && self.drag_src.is_none() {
            // The leave message can be lost around a drag. Trust the real cursor.
            let c = cursor();
            for k in 0..self.wins.len() {
                if self.wins[k].mouse_in && !in_rect(&self.wins[k].win, c) && self.wins[k].drag == crate::folder::Drag::None {
                    self.on_leave(k);
                }
            }
            self.close_if_abandoned();
        }
        let busy = self.any_animating() || self.drag_src.is_some() || self.popup.is_some() || self.prefs.is_some()
            || self.wins.iter().any(|w| w.mouse_in || w.edit.is_some() || w.target > 0.0 && w.cfg.mode == Mode::Hover);
        if busy {
            self.arm_idle();
            return;
        }
        for w in self.wins.iter_mut() {
            w.release();
        }
        self.ghost_cv = None;
        self.fonts.clear();
        unsafe {
            HeapCompact(GetProcessHeap(), 0);
            K32EmptyWorkingSet(GetCurrentProcess());
        }
        self.trimmed = true;
    }

    /// Every 20 seconds: if memory crept back above 1 MB while nothing was
    /// happening, trim it again. Costs a few microseconds.
    pub fn on_watch(&mut self) {
        let mut c = PROCESS_MEMORY_COUNTERS::default();
        c.cb = core::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0;
        if ok && c.WorkingSetSize > 1 << 20 {
            self.on_idle();
        }
        // this wakeup should not schedule another trim by itself
        self.trimmed = true;
    }

    pub fn save(&mut self) {
        // Remember where each folder sits on this PC. Other computers keep their own positions.
        for w in self.wins.iter_mut() {
            let (x, y) = (w.cfg.x, w.cfg.y);
            match w.cfg.pos.iter_mut().find(|p| p.0.eq_ignore_ascii_case(&self.pc)) {
                Some(p) => {
                    p.1 = x;
                    p.2 = y;
                }
                None => w.cfg.pos.push((self.pc.clone(), x, y)),
            }
        }
        let cfg = Config { g: self.g.clone(), folders: self.wins.iter().map(|w| w.cfg.clone()).collect() };
        let b = cfg.serialize().into_bytes();
        if b != self.last_ini {
            write_file(&self.ini, &b);
            self.last_ini = b;
        }
    }

    /// Applies crow.ini right away when it changes on disk.
    pub fn reload(&mut self) {
        let Some(d) = read_file_max(&self.ini, 2 << 20) else { return self.rescan_all() };
        if d == self.last_ini || d.is_empty() {
            // The settings did not change, but shortcuts may have arrived or left through sync
            return self.rescan_all();
        }
        let mut cfg = Config::parse(core::str::from_utf8(&d).unwrap_or(""));
        for f in cfg.folders.iter_mut() {
            use_pc_pos(f, &self.pc);
        }
        self.last_ini = d;
        let autostart_changed = cfg.g.autostart != self.g.autostart;
        if cfg.g.language != self.g.language {
            crate::i18n::set(&cfg.g.language);
        }
        let menu_changed = cfg.g.desktop_menu != self.g.desktop_menu;
        self.g = cfg.g;
        self.fonts.clear();
        // Close windows whose folder no longer exists
        let mut i = 0;
        while i < self.wins.len() {
            if cfg.folders.iter().any(|f| f.id == self.wins[i].cfg.id) {
                i += 1;
            } else {
                self.remove_folder(i);
            }
        }
        for f in cfg.folders {
            match self.by_id(f.id) {
                Some(i) => {
                    let w = &mut self.wins[i];
                    let mode_changed = w.cfg.mode != f.mode;
                    w.cfg = f;
                    if mode_changed {
                        let p = if w.cfg.mode == Mode::Panel { 1.0 } else { 0.0 };
                        w.t = p;
                        w.target = p;
                    }
                    w.rescan();
                }
                None => {
                    self.add_folder(f);
                }
            }
        }
        if autostart_changed {
            store::set_autostart(self.g.autostart);
        }
        if menu_changed {
            store::set_desktop_menu(self.g.desktop_menu);
        }
        self.relayout_all();
    }

    /// Reads the folder directories again to pick up shortcuts added or removed by sync.
    pub fn rescan_all(&mut self) {
        let mut changed = false;
        for i in 0..self.wins.len() {
            if self.wins[i].dir_changed() {
                let w = &mut self.wins[i];
                w.rescan();
                w.place(&self.g);
                self.render(i);
                changed = true;
            }
        }
        if changed {
            self.save();
        }
    }

    pub fn snap(&self, v: i32) -> i32 {
        let s = self.g.snap;
        if s <= 1 || unsafe { GetKeyState(VK_SHIFT) } < 0 {
            v
        } else {
            ((v as f32 / s as f32) + 0.5) as i32 * s
        }
    }

    pub fn next_id(&self) -> u32 {
        self.wins.iter().map(|w| w.cfg.id).max().unwrap_or(0) + 1
    }

    pub fn folder_at(&self, p: POINT) -> Option<usize> {
        // Prefer open folders, since they sit on top
        let hit = |w: &FolderWin| {
            in_rect(&w.icon_box, p) || (w.expanded() && in_rect(&w.panel, p))
        };
        self.wins.iter().position(|w| w.expanded() && w.cfg.mode == Mode::Hover && hit(w))
            .or_else(|| self.wins.iter().position(|w| hit(w)))
    }

    pub fn clear_drop_marks(&mut self) {
        for i in 0..self.wins.len() {
            let w = &mut self.wins[i];
            if w.drop_hover || w.insert_at >= 0 {
                w.drop_hover = false;
                w.insert_at = -1;
                if !w.animating() {
                    self.render(i);
                }
            }
        }
    }
}

/// Uses the position saved for this computer when there is one. Otherwise the
/// shared x and y are used, and `place()` keeps the folder on screen.
fn use_pc_pos(f: &mut Folder, pc: &str) {
    if let Some(p) = f.pos.iter().find(|p| p.0.eq_ignore_ascii_case(pc)) {
        f.x = p.1;
        f.y = p.2;
    }
}

fn data_missing(d: &[u8]) -> bool {
    d.is_empty()
}

fn default_config() -> Config {
    let mut c = Config::parse("");
    let mut wa = RECT::default();
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut _ as *mut core::ffi::c_void, 0) };
    let sc = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let x = wa.right - (140.0 * sc) as i32;
    let y = wa.top + (40.0 * sc) as i32;
    let mut f = Folder::new(1, crate::i18n::tr("default.programs"), x, y);
    f.color = 0xFFF5B841;
    c.folders.push(f);
    let mut f2 = Folder::new(2, crate::i18n::tr("default.games"), x, y + (130.0 * sc) as i32);
    f2.color = 0xFF7C6CFF;
    c.folders.push(f2);
    c
}
