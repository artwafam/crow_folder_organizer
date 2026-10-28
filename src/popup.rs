//! Floating windows drawn by Crow: the context menu, the folder settings
//! popover and the Crow settings window. They share one layered window class
//! and the same UI toolkit. The menu and the popover close as soon as the user
//! clicks somewhere else. The settings window behaves like a normal window.

use crate::app::*;
use crate::edit::{Edit, Key};
use crate::ffi::*;
use crate::gfx::*;
use crate::i18n::tr;
use crate::uikit::*;
use crate::util::*;
use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq)]
pub enum Act {
    Sep,
    Open,
    Reveal,
    RenameItem,
    ItemIcon,
    ItemIconReset,
    Export,
    Delete,
    Settings,
    RenameFolder,
    Collapse,
    NewFolder,
    NewPanel,
    AppSettings,
}

pub struct MenuSpec {
    pub folder: u32,
    pub item: Option<usize>,
    pub entries: Vec<(Act, Ic, &'static str, bool)>,
    /// Entry highlighted with the keyboard, or usize::MAX.
    pub key: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Pick {
    Folder,
    Tint,
    Background,
    DefaultColor,
}

pub struct SettingsState {
    pub folder: u32,
    pub picker: Option<(Pick, f32, f32, f32)>,
    pub name: Edit,
    pub hex: Edit,
    /// Which text field has the keyboard: 0 none, 1 name, 2 hex.
    pub focus: u8,
    pub confirm_delete: f64,
    /// Values shown while a size slider is dragged (applied on release).
    pub preview: Option<(u32, f32)>,
}

pub struct PrefsState {
    pub page: usize,
    pub picker: Option<(Pick, f32, f32, f32)>,
    pub hex: Edit,
    pub focus: u8,
    pub preview: Option<(u32, f32)>,
}

pub enum Kind {
    Menu(MenuSpec),
    Settings(SettingsState),
    Prefs(PrefsState),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Slot {
    Pop,
    Prefs,
}

pub struct Popup {
    pub hwnd: HWND,
    pub kind: Kind,
    pub sc: f32,
    /// Content rectangle on screen (without the shadow margin).
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub anchor: RECT,
    pub cv: Option<Canvas>,
    pub texts: Texts,
    pub icons: Icons,
    pub anim: Anim,
    pub fonts: Fonts,
    pub inp: Input,
    pub active: u32,
    pub t: f32,
    pub closing: bool,
    pub scroll: f32,
    pub content_h: f32,
    pub modal: bool,
    pub cursor: usize,
    pub result: Option<Act>,
}

pub const MARGIN: f32 = 18.0;
/// Height of the settings window title bar (logical px).
pub const PREFS_BAR: f32 = 52.0;
pub const PREFS_SIDE: f32 = 190.0;

unsafe extern "system" fn popup_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let a = app();
    match a.popup_msg(h, m, w, l) {
        Some(r) => r,
        None => DefWindowProcW(h, m, w, l),
    }
}

pub fn register_class() {
    unsafe {
        let inst = GetModuleHandleW(core::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: popup_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: LoadIconW(inst, 1),
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: 0,
            lpszMenuName: core::ptr::null(),
            lpszClassName: w!("CrowPopup").as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc);
    }
}

pub fn work_area(p: POINT) -> RECT {
    unsafe {
        let mut mi = MONITORINFO { cbSize: core::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST), &mut mi);
        mi.rcWork
    }
}

impl Popup {
    pub fn new(kind: Kind, sc: f32) -> Box<Popup> {
        Box::new(Popup {
            hwnd: 0,
            kind,
            sc,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            anchor: RECT::default(),
            cv: None,
            texts: Texts::new(),
            icons: Icons::new(),
            anim: Anim::new(),
            fonts: Fonts::new(),
            inp: Input::default(),
            active: 0,
            t: 0.0,
            closing: false,
            scroll: 0.0,
            content_h: 0.0,
            modal: false,
            cursor: IDC_ARROW,
            result: None,
        })
    }
    pub fn folder(&self) -> u32 {
        match &self.kind {
            Kind::Menu(m) => m.folder,
            Kind::Settings(s) => s.folder,
            Kind::Prefs(_) => 0,
        }
    }
    fn focus(&self) -> u8 {
        match &self.kind {
            Kind::Settings(s) => s.focus,
            Kind::Prefs(s) => s.focus,
            Kind::Menu(_) => 0,
        }
    }
    pub fn animating(&self) -> bool {
        (self.t < 1.0 && !self.closing) || (self.closing && self.t > 0.0) || self.anim.busy() || self.focus() != 0
    }
    /// Top of the scrolling content, relative to the popup's content box.
    fn content_top(&self) -> f32 {
        match self.kind {
            Kind::Prefs(_) => PREFS_BAR * self.sc,
            _ => 6.0 * self.sc,
        }
    }
}

impl App {
    fn slot(&mut self, s: Slot) -> &mut Option<Box<Popup>> {
        match s {
            Slot::Pop => &mut self.popup,
            Slot::Prefs => &mut self.prefs,
        }
    }
    fn slot_of(&self, h: HWND) -> Option<Slot> {
        if self.popup.as_ref().is_some_and(|p| p.hwnd == h) {
            Some(Slot::Pop)
        } else if self.prefs.as_ref().is_some_and(|p| p.hwnd == h) {
            Some(Slot::Prefs)
        } else {
            None
        }
    }

    pub fn popup_open_for(&self, folder: u32) -> bool {
        self.popup.as_ref().is_some_and(|p| p.folder() == folder && !p.closing)
    }

    pub fn open_menu(&mut self, i: usize, item: Option<usize>, at: POINT) {
        self.close_popup(false);
        let p = self.make_menu(i, item, at);
        self.show(Slot::Pop, p);
    }

    pub fn make_menu(&self, i: usize, item: Option<usize>, at: POINT) -> Box<Popup> {
        let f = &self.wins[i];
        let mut entries: Vec<(Act, Ic, &'static str, bool)> = Vec::new();
        if let Some(j) = item {
            let custom = f.cfg.item_icon(&from_w(&f.items[j])).is_some();
            entries.push((Act::Open, Ic::Open, tr("menu.open"), false));
            entries.push((Act::Reveal, Ic::Folder, tr("menu.reveal"), false));
            entries.push((Act::Sep, Ic::Plus, "", false));
            entries.push((Act::RenameItem, Ic::Rename, tr("menu.rename"), false));
            entries.push((Act::ItemIcon, Ic::Image, tr("menu.change_icon"), false));
            if custom {
                entries.push((Act::ItemIconReset, Ic::Reset, tr("menu.reset_icon"), false));
            }
            entries.push((Act::Sep, Ic::Plus, "", false));
            entries.push((Act::Export, Ic::Out, tr("menu.to_desktop"), false));
            entries.push((Act::Delete, Ic::Trash, tr("menu.delete"), true));
        } else {
            entries.push((Act::Settings, Ic::Gear, tr("menu.settings"), false));
            entries.push((Act::RenameFolder, Ic::Rename, tr("menu.rename"), false));
            if f.cfg.mode == crate::config::Mode::Panel {
                let (ic, label) = if f.cfg.collapsed { (Ic::ChevronDown, tr("menu.expand")) } else { (Ic::ChevronUp, tr("menu.collapse")) };
                entries.push((Act::Collapse, ic, label, false));
            }
            entries.push((Act::Sep, Ic::Plus, "", false));
            entries.push((Act::NewFolder, Ic::Plus, tr("menu.new_folder"), false));
            entries.push((Act::NewPanel, Ic::Panel, tr("menu.new_panel"), false));
            entries.push((Act::Sep, Ic::Plus, "", false));
            entries.push((Act::AppSettings, Ic::Palette, tr("menu.app_settings"), false));
        }
        let sc = f.sc;
        let n_items = entries.iter().filter(|e| e.0 != Act::Sep).count() as f32;
        let n_seps = entries.iter().filter(|e| e.0 == Act::Sep).count() as f32;
        let mut p = Popup::new(Kind::Menu(MenuSpec { folder: f.cfg.id, item, entries, key: usize::MAX }), sc);
        p.w = (248.0 * sc) as i32;
        p.h = ((n_items * 34.0 + n_seps * 10.0 + 12.0) * sc) as i32;
        let wa = work_area(at);
        p.x = at.x.min(wa.right - p.w - 4).max(wa.left + 4);
        p.y = if at.y + p.h > wa.bottom - 4 { (at.y - p.h).max(wa.top + 4) } else { at.y };
        p
    }

    pub fn open_settings(&mut self, i: usize) {
        self.close_popup(false);
        let p = self.make_settings(i);
        self.show(Slot::Pop, p);
    }

    pub fn make_settings(&mut self, i: usize) -> Box<Popup> {
        let f = &self.wins[i];
        let sc = f.sc;
        let st = SettingsState {
            folder: f.cfg.id,
            picker: None,
            name: Edit::new(&to_w(&f.cfg.name), 80, false),
            hex: Edit::new(&[], 7, false),
            focus: 0,
            confirm_delete: 0.0,
            preview: None,
        };
        let mut p = Popup::new(Kind::Settings(st), sc);
        p.w = (348.0 * sc) as i32;
        // anchor to the whole panel, so the popover can sit beside it
        p.anchor = f.panel;
        p.h = (420.0 * sc) as i32;
        // measure the real content height with a dry run
        self.popup = Some(p);
        self.pass(Slot::Pop, false);
        let mut p = self.popup.take().unwrap();
        self.fit(&mut p);
        p
    }

    /// Opens the Crow settings window, or brings it to the front.
    pub fn open_prefs(&mut self) {
        if let Some(p) = self.prefs.as_ref() {
            unsafe {
                ShowWindow(p.hwnd, SW_SHOW);
                SetForegroundWindow(p.hwnd);
            }
            return;
        }
        self.close_popup(false);
        let p = self.make_prefs();
        self.show(Slot::Prefs, p);
    }

    pub fn make_prefs(&mut self) -> Box<Popup> {
        let c = cursor();
        let wa = work_area(c);
        let sc = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
        let st = PrefsState { page: 0, picker: None, hex: Edit::new(&[], 7, false), focus: 0, preview: None };
        let mut p = Popup::new(Kind::Prefs(st), sc);
        p.w = ((680.0 * sc) as i32).min(wa.right - wa.left - 40);
        p.h = ((560.0 * sc) as i32).min(wa.bottom - wa.top - 40);
        p.x = wa.left + (wa.right - wa.left - p.w) / 2;
        p.y = wa.top + (wa.bottom - wa.top - p.h) / 2;
        p
    }

    /// Sizes the folder popover and places it beside the folder (right side
    /// first, then left), so every change stays visible while adjusting it.
    fn fit(&self, p: &mut Popup) {
        if !matches!(p.kind, Kind::Settings(_)) {
            return;
        }
        let a = p.anchor;
        let wa = work_area(POINT { x: (a.left + a.right) / 2, y: a.top });
        let sc = p.sc;
        let max_h = ((wa.bottom - wa.top) as f32 - 24.0 * sc).min(640.0 * sc);
        p.h = (p.content_top() + p.content_h + 6.0 * sc).min(max_h) as i32;
        let gap = (12.0 * sc) as i32;
        let edge = (8.0 * sc) as i32;
        if a.right + gap + p.w <= wa.right - edge {
            p.x = a.right + gap;
        } else if a.left - gap - p.w >= wa.left + edge {
            p.x = a.left - gap - p.w;
        } else {
            p.x = (a.right - p.w).min(wa.right - p.w - edge).max(wa.left + edge);
        }
        p.y = a.top.min(wa.bottom - p.h - edge).max(wa.top + edge);
        let max_scroll = (p.content_h - (p.h as f32 - p.content_top() - 6.0 * sc)).max(0.0);
        p.scroll = p.scroll.min(max_scroll);
    }

    fn show(&mut self, s: Slot, mut p: Box<Popup>) {
        unsafe {
            let m = (MARGIN * p.sc) as i32;
            let (ex, title) = match s {
                Slot::Pop => (WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("Crow FO").as_ptr()),
                Slot::Prefs => (WS_EX_LAYERED | WS_EX_APPWINDOW, wz(&to_w(tr("prefs.title"))).leak().as_ptr()),
            };
            p.hwnd = CreateWindowExW(ex, w!("CrowPopup").as_ptr(), title,
                WS_POPUP, p.x - m, p.y - m, p.w + 2 * m, p.h + 2 * m, 0, 0, GetModuleHandleW(core::ptr::null()), core::ptr::null());
            let hwnd = p.hwnd;
            *self.slot(s) = Some(p);
            self.pass(s, true);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
        }
        self.was_animating = false;
    }

    pub fn close_popup(&mut self, animate: bool) {
        self.close(Slot::Pop, animate);
    }
    pub fn close_prefs(&mut self) {
        self.close(Slot::Prefs, true);
    }

    fn close(&mut self, s: Slot, animate: bool) {
        let Some(p) = self.slot(s).as_mut() else { return };
        if animate && !p.closing {
            p.closing = true;
            self.was_animating = false;
            return;
        }
        let p = self.slot(s).take().unwrap();
        unsafe { DestroyWindow(p.hwnd) };
        let fid = p.folder();
        if let Some(i) = self.by_id(fid) {
            self.wins[i].hover = -1;
            self.wins[i].gear_on = false;
            self.render(i);
            let f = &self.wins[i];
            if !f.mouse_in && f.cfg.mode == crate::config::Mode::Hover && f.target > 0.0 {
                unsafe { SetTimer(f.hwnd, T_CLOSE, self.g.close_delay.max(1), 0) };
            }
        }
        self.save();
    }

    /// Advances popup animations. Returns true while something still moves.
    pub fn popup_step(&mut self, dt: f32) -> bool {
        let mut busy = false;
        for s in [Slot::Pop, Slot::Prefs] {
            let Some(p) = self.slot(s).as_mut() else { continue };
            let speed = if p.closing { 90.0 } else { 140.0 };
            if p.closing {
                p.t = (p.t - dt / speed).max(0.0);
            } else {
                p.t = (p.t + dt / speed).min(1.0);
            }
            let hover = p.anim.step(dt);
            if p.closing && p.t <= 0.0 {
                let act = p.result.take();
                let fid = p.folder();
                let item = match &p.kind {
                    Kind::Menu(m) => m.item,
                    _ => None,
                };
                self.close(s, false);
                if let Some(a) = act {
                    self.run_menu_act(a, fid, item);
                }
                continue;
            }
            if hover || p.focus() != 0 {
                self.pass(s, true);
            } else {
                self.present(s);
            }
            if let Some(p) = self.slot(s).as_ref() {
                busy |= p.animating();
            }
        }
        busy
    }

    fn present(&mut self, s: Slot) {
        let Some(p) = self.slot(s).as_ref() else { return };
        let Some(cv) = p.cv.as_ref() else { return };
        let e = if p.closing { p.t } else { ease_out_cubic(p.t) };
        let m = (MARGIN * p.sc) as i32;
        let dy = if s == Slot::Pop { ((1.0 - e) * -6.0 * p.sc) as i32 } else { ((1.0 - e) * 10.0 * p.sc) as i32 };
        unsafe {
            let pos = POINT { x: p.x - m, y: p.y - m + dy };
            let size = SIZE { cx: cv.w, cy: cv.h };
            let src = POINT::default();
            let bf = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: (e * 255.0) as u8, AlphaFormat: AC_SRC_ALPHA };
            UpdateLayeredWindow(p.hwnd, 0, &pos, &size, cv.dc, &src, 0, &bf, ULW_ALPHA);
        }
    }

    /// Kept for callers that only deal with the menu and popover.
    pub fn popup_pass(&mut self, draw: bool) {
        self.pass(Slot::Pop, draw);
    }

    /// Runs a popup's UI once. With `draw`, it renders and presents.
    pub fn pass(&mut self, s: Slot, draw: bool) {
        let Some(mut p) = self.slot(s).take() else { return };
        let sc = p.sc;
        let m = MARGIN * sc;
        let (cw, ch) = (p.w + 2 * m as i32, p.h + 2 * m as i32);
        if draw && !matches!(&p.cv, Some(c) if c.w == cw && c.h == ch) {
            p.cv = Canvas::dib(cw, ch);
        }
        let accent = match &p.kind {
            Kind::Prefs(_) => self.wins.first().map_or(0xFFF5B841, |w| w.cfg.color),
            _ => self.by_id(p.folder()).map_or(0xFF5B8CFF, |i| self.wins[i].cfg.color),
        };
        let th = Theme::dark(accent);
        let radius = if s == Slot::Prefs { 14.0 } else { 12.0 };
        let mut cvo = if draw { p.cv.take() } else { None };
        if let Some(cv) = cvo.as_mut() {
            cv.reset_clip();
            cv.clear(0);
            let (w, h) = (p.w as f32, p.h as f32);
            cv.shadow(m, m + 6.0 * sc, w, h, radius * sc, 0x66000000, 255, 18.0 * sc);
            cv.rrect(m, m, w, h, radius * sc, th.bg, th.bg, 255, 1.0);
            cv.rrect_stroke(m, m, w, h, radius * sc, 1.0 * sc, th.border, th.border, 255);
        }
        let face = self.g.font.clone();
        let mut inp = p.inp;
        inp.mx += m;
        inp.my += m;
        let (pw, ph) = (p.w as f32, p.h as f32);
        let top = m + p.content_top();
        let mut menu_result = None;
        let mut close_req = false;
        let Popup { kind, anim, texts, icons, fonts, active, scroll, content_h, cursor, .. } = &mut *p;
        {
            let mut ui = Ui {
                cv: cvo.as_mut(),
                sc,
                x: m + 6.0 * sc,
                y: m + 6.0 * sc,
                w: pw - 12.0 * sc,
                inp,
                active,
                anim,
                texts,
                icons,
                fonts,
                face: &face,
                th,
                cursor: IDC_ARROW,
                view: (m, m + ph),
                released_id: 0,
            };
            match kind {
                Kind::Menu(spec) => {
                    menu_result = menu_ui(&mut ui, spec);
                }
                Kind::Settings(st) => {
                    let pad = 16.0 * sc;
                    let bottom = m + ph - 6.0 * sc;
                    ui.view = (top, bottom);
                    if let Some(cv) = ui.cv.as_deref_mut() {
                        cv.set_clip(0, top as i32, cv.w, bottom as i32);
                    }
                    ui.x = m + pad;
                    ui.w = pw - pad * 2.0;
                    ui.y = top - *scroll;
                    let start = ui.y;
                    crate::settings::build(self, st, &mut ui);
                    *content_h = ui.y - start + 8.0 * sc;
                    scrollbar(&mut ui, *content_h, *scroll, top, bottom, m + pw - 6.0 * sc);
                }
                Kind::Prefs(st) => {
                    close_req = crate::prefs::build(self, st, &mut ui, m, pw, ph, *scroll, content_h);
                }
            }
            *cursor = ui.cursor;
        }
        if menu_result.is_some() {
            p.result = menu_result;
        }
        if p.inp.released {
            p.active = 0;
        }
        p.inp.pressed = false;
        p.inp.released = false;
        p.inp.dbl = false;
        if draw {
            p.cv = cvo;
        }
        // the popover may have changed size (color picker)
        if let Kind::Settings(_) = &p.kind {
            let old = (p.x, p.y, p.h);
            self.fit(&mut p);
            if draw && (p.x, p.y, p.h) != old {
                *self.slot(s) = Some(p);
                return self.pass(s, true);
            }
        }
        if p.result.is_some() && !p.closing {
            p.closing = true;
        }
        *self.slot(s) = Some(p);
        if close_req {
            self.close(s, true);
        }
        if draw {
            self.present(s);
        }
    }

    pub fn popup_msg(&mut self, h: HWND, m: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
        let s = self.slot_of(h)?;
        let p = self.slot(s).as_mut().unwrap();
        let sc_m = (MARGIN * p.sc) as i32;
        let pt = |l: LPARAM| {
            let q = lp_pt(l);
            ((q.x - sc_m) as f32, (q.y - sc_m) as f32)
        };
        match m {
            WM_NCHITTEST if s == Slot::Prefs => {
                // the title bar drags the settings window, except over the close button
                let q = lp_pt(l);
                let (x, y) = ((q.x - p.x) as f32, (q.y - p.y) as f32);
                let bar = PREFS_BAR * p.sc;
                if y >= 0.0 && y < bar && x >= 0.0 && x < p.w as f32 - bar {
                    return Some(HTCAPTION);
                }
                return Some(HTCLIENT);
            }
            WM_MOVE if s == Slot::Prefs => {
                let mut r = RECT::default();
                unsafe { GetWindowRect(h, &mut r) };
                p.x = r.left + sc_m;
                p.y = r.top + sc_m;
                return None;
            }
            WM_MOUSEMOVE => {
                let (x, y) = pt(l);
                if !p.inp.inside {
                    let mut t = TRACKMOUSEEVENT { cbSize: core::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    unsafe { TrackMouseEvent(&mut t) };
                }
                p.inp.mx = x;
                p.inp.my = y;
                p.inp.inside = true;
                if let Kind::Menu(st) = &mut p.kind {
                    st.key = usize::MAX;
                }
                self.pass(s, true);
            }
            WM_MOUSELEAVE => {
                p.inp.inside = false;
                self.pass(s, true);
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let (x, y) = pt(l);
                p.inp.mx = x;
                p.inp.my = y;
                p.inp.inside = true;
                p.inp.down = true;
                p.inp.pressed = true;
                p.inp.dbl = m == WM_LBUTTONDBLCLK;
                unsafe { SetCapture(h) };
                // clicking outside a text field takes the keyboard away from it
                match &mut p.kind {
                    Kind::Settings(st) => st.focus = 0,
                    Kind::Prefs(st) => st.focus = 0,
                    _ => {}
                }
                self.pass(s, false);
                self.pass(s, true);
            }
            WM_LBUTTONUP => {
                let (x, y) = pt(l);
                p.inp.mx = x;
                p.inp.my = y;
                p.inp.down = false;
                p.inp.released = true;
                unsafe { ReleaseCapture() };
                self.pass(s, false);
                self.pass(s, true);
            }
            WM_MOUSEWHEEL => {
                let d = ((w >> 16) & 0xFFFF) as i16 as f32 / 120.0;
                if !matches!(p.kind, Kind::Menu(_)) {
                    let view = p.h as f32 - p.content_top() - 6.0 * p.sc;
                    let max = (p.content_h - view).max(0.0);
                    p.scroll = clamp(p.scroll - d * 48.0 * p.sc, 0.0, max);
                    self.pass(s, true);
                }
            }
            WM_SETCURSOR => {
                if (l & 0xFFFF) as isize == HTCAPTION {
                    return None;
                }
                unsafe { SetCursor(LoadCursorW(0, p.cursor)) };
                return Some(1);
            }
            WM_ACTIVATE => {
                if s == Slot::Pop && (w & 0xFFFF) == WA_INACTIVE && !p.modal {
                    self.close(s, true);
                }
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => self.popup_key(s, w),
            WM_CHAR => {
                let r = match &mut p.kind {
                    Kind::Settings(st) => match st.focus {
                        1 => st.name.on_char(w as u16),
                        2 => st.hex.on_char(w as u16),
                        _ => Key::None,
                    },
                    Kind::Prefs(st) => if st.focus == 2 { st.hex.on_char(w as u16) } else { Key::None },
                    _ => Key::None,
                };
                if matches!(r, Key::Changed) {
                    self.text_changed(s);
                }
                self.pass(s, true);
            }
            WM_CLOSE => {
                self.close(s, true);
            }
            WM_MOUSEACTIVATE => return Some(MA_ACTIVATE),
            _ => return None,
        }
        Some(0)
    }

    fn popup_key(&mut self, s: Slot, vk: usize) {
        let Some(p) = self.slot(s).as_mut() else { return };
        let hwnd = p.hwnd;
        match &mut p.kind {
            Kind::Menu(st) => {
                let n = st.entries.len();
                let mut step = |dir: isize| {
                    let mut k = if st.key == usize::MAX { if dir > 0 { n - 1 } else { 0 } } else { st.key };
                    for _ in 0..n {
                        k = ((k as isize + dir).rem_euclid(n as isize)) as usize;
                        if st.entries[k].0 != Act::Sep {
                            break;
                        }
                    }
                    st.key = k;
                };
                match vk {
                    VK_DOWN => step(1),
                    VK_UP => step(-1),
                    VK_ESCAPE => {
                        self.close(s, true);
                        return;
                    }
                    VK_RETURN if st.key < n => {
                        p.result = Some(st.entries[st.key].0);
                        p.closing = true;
                    }
                    _ => {}
                }
                self.pass(s, true);
            }
            Kind::Settings(st) => {
                let r = match st.focus {
                    1 => st.name.on_key(vk, hwnd),
                    2 => st.hex.on_key(vk, hwnd),
                    _ => {
                        if vk == VK_ESCAPE {
                            self.close(s, true);
                            return;
                        }
                        Key::None
                    }
                };
                match r {
                    Key::Commit => {
                        st.focus = 0;
                        self.text_changed(s);
                    }
                    Key::Cancel => {
                        if st.focus == 1 {
                            let fid = st.folder;
                            let name = self.by_id(fid).map(|i| to_w(&self.wins[i].cfg.name)).unwrap_or_default();
                            if let Some(Popup { kind: Kind::Settings(st), .. }) = self.popup.as_deref_mut() {
                                st.name = Edit::new(&name, 80, false);
                                st.focus = 0;
                            }
                        } else {
                            st.focus = 0;
                        }
                    }
                    Key::Changed => self.text_changed(s),
                    _ => {}
                }
                self.pass(s, true);
            }
            Kind::Prefs(st) => {
                if st.focus == 2 {
                    match st.hex.on_key(vk, hwnd) {
                        Key::Commit | Key::Cancel => st.focus = 0,
                        Key::Changed => self.text_changed(s),
                        _ => {}
                    }
                } else if vk == VK_ESCAPE {
                    self.close(s, true);
                    return;
                }
                self.pass(s, true);
            }
        }
    }

    fn text_changed(&mut self, s: Slot) {
        if s == Slot::Pop {
            self.settings_text_changed();
        } else {
            crate::prefs::hex_changed(self);
        }
    }
}

fn scrollbar(ui: &mut Ui, content_h: f32, scroll: f32, top: f32, bottom: f32, x: f32) {
    let view_h = bottom - top;
    if content_h > view_h {
        let sc = ui.sc;
        let frac = view_h / content_h;
        let pos = scroll / (content_h - view_h);
        let bh = view_h * frac;
        let by = top + (view_h - bh) * pos;
        if let Some(cv) = ui.cv.as_deref_mut() {
            cv.rrect(x, by, 3.0 * sc, bh, 1.5 * sc, 0x50FFFFFF, 0x50FFFFFF, 255, 1.0);
        }
    }
}

pub fn draw_scrollbar(ui: &mut Ui, content_h: f32, scroll: f32, top: f32, bottom: f32, x: f32) {
    scrollbar(ui, content_h, scroll, top, bottom, x);
}

fn menu_ui(ui: &mut Ui, spec: &mut MenuSpec) -> Option<Act> {
    let mut res = None;
    for k in 0..spec.entries.len() {
        let (act, ic, label, danger) = spec.entries[k];
        if act == Act::Sep {
            ui.separator();
            continue;
        }
        if ui.menu_item(7000 + k as u32, ic, label, danger, spec.key == k) {
            res = Some(act);
        }
    }
    if res.is_some() || ui.cursor != IDC_ARROW {
        ui.cursor = IDC_ARROW;
    }
    res
}
