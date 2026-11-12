//! Windows, messages, timers, mouse and keyboard on X11.
//!
//! Every Crow window is an X window with a 32-bit visual. Its pixels come from
//! UpdateLayeredWindow, and clicks pass through fully transparent pixels like
//! they do on Windows (the X input shape follows the alpha channel).

use super::x::{x, X};
use super::*;
use std::collections::{HashMap, VecDeque};
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    /// Never shown, receives posted messages and timers.
    Host,
    /// Folders and panels: below normal windows, no focus unless editing.
    Desk,
    /// Menus and the folder settings popover.
    Popup,
    /// The settings window, with a taskbar entry.
    App,
    /// Follows the pointer during a drag, never takes input.
    Ghost,
}

struct Class {
    name: Vec<u16>,
    proc_: WNDPROC,
    style: u32,
    cursor: HCURSOR,
}

pub struct Win {
    pub id: u32,
    proc_: WNDPROC,
    class_style: u32,
    class_cursor: HCURSOR,
    pub role: Role,
    pub ex: u32,
    user: isize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub visible: bool,
    mapped: bool,
    track_leave: bool,
    inside: bool,
    gc: Gcontext,
    /// The image lives on the X server as the window background, so the server
    /// repaints it by itself and Crow keeps no copy (like a layered window).
    pixmap: u32,
    pw: i32,
    ph: i32,
    shape_hash: u64,
    want_focus: bool,
    pub dnd: bool,
    /// The band last asked for with set_layer.
    layer: Option<i32>,
}

struct Timer {
    hwnd: HWND,
    id: usize,
    ms: u32,
    due: f64,
}

pub type ClientHook = fn(&Event) -> bool;

struct State {
    classes: Vec<Class>,
    wins: HashMap<u32, Win>,
    queue: VecDeque<MSG>,
    timers: Vec<Timer>,
    capture: HWND,
    hover: HWND,
    focus: HWND,
    active: HWND,
    cursor: HCURSOR,
    fg_hook: Option<WINEVENTPROC>,
    last_click: (HWND, f64, i32, i32),
    keymap: Vec<u32>,
    per: usize,
    min_kc: u8,
    dead: Option<char>,
    cursors: HashMap<usize, u32>,
    monitors: Option<Vec<(RECT, bool)>>,
    next_msg: u32,
    client_hook: Option<ClientHook>,
    showing_desktop: bool,
    quit: bool,
    last_workarea: Option<RECT>,
    /// What the loop handled since it last slept, for the watchdog.
    seen: HashMap<String, u32>,
    busy_since: f64,
}

static mut S: Option<State> = None;

fn st() -> &'static mut State {
    unsafe {
        let s = &mut *core::ptr::addr_of_mut!(S);
        s.get_or_insert_with(|| State {
            classes: Vec::new(),
            wins: HashMap::new(),
            queue: VecDeque::new(),
            timers: Vec::new(),
            capture: 0,
            hover: 0,
            focus: 0,
            active: 0,
            cursor: 0,
            fg_hook: None,
            last_click: (0, 0.0, 0, 0),
            keymap: Vec::new(),
            per: 0,
            min_kc: 8,
            dead: None,
            cursors: HashMap::new(),
            monitors: None,
            next_msg: 0xC000,
            client_hook: None,
            showing_desktop: false,
            quit: false,
            last_workarea: None,
            seen: HashMap::new(),
            busy_since: 0.0,
        })
    }
}

fn now() -> f64 {
    crate::util::now_ms()
}

pub fn win(h: HWND) -> Option<&'static mut Win> {
    st().wins.get_mut(&(h as u32))
}

pub fn is_ours(h: HWND) -> bool {
    h != 0 && st().wins.contains_key(&(h as u32))
}

/// Called for X client messages and selection events the window layer
/// doesn't handle itself (drag and drop).
pub fn set_client_hook(f: ClientHook) {
    st().client_hook = Some(f);
}

fn pack(x: i32, y: i32) -> LPARAM {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as i32 as LPARAM
}

unsafe fn call(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(wd) = win(h) else { return 0 };
    let p = wd.proc_;
    p(h, m, w, l)
}

// ------------------------------------------------------------------ classes
pub unsafe extern "system" fn RegisterClassExW(c: *const WNDCLASSEXW) -> u16 {
    let c = &*c;
    let name = wstr(c.lpszClassName);
    let s = st();
    s.classes.retain(|k| k.name != name);
    s.classes.push(Class { name, proc_: c.lpfnWndProc, style: c.style, cursor: c.hCursor });
    s.classes.len() as u16
}

pub unsafe extern "system" fn RegisterWindowMessageW(_s: PCWSTR) -> u32 {
    let s = st();
    s.next_msg += 1;
    s.next_msg
}

pub unsafe extern "system" fn DefWindowProcW(h: HWND, m: u32, _w: WPARAM, _l: LPARAM) -> LRESULT {
    match m {
        WM_SETCURSOR => {
            if let Some(wd) = win(h) {
                SetCursor(wd.class_cursor);
            }
            1
        }
        WM_NCHITTEST => HTCLIENT,
        WM_MOUSEACTIVATE => MA_ACTIVATE,
        _ => 0,
    }
}

// ------------------------------------------------------------------ windows
fn role_for(class: &[u16], ex: u32) -> Role {
    if crate::util::weq_ic(class, w!("CrowFOHost")) || ex & WS_EX_LAYERED == 0 {
        Role::Host
    } else if ex & WS_EX_TRANSPARENT != 0 {
        Role::Ghost
    } else if ex & WS_EX_APPWINDOW != 0 {
        Role::App
    } else if ex & WS_EX_NOACTIVATE != 0 {
        Role::Desk
    } else {
        Role::Popup
    }
}

pub unsafe extern "system" fn CreateWindowExW(ex: u32, cls: PCWSTR, name: PCWSTR, _style: u32, x0: i32, y0: i32, w0: i32, h0: i32,
    _parent: HWND, _menu: HMENU, _inst: HINSTANCE, _param: *const c_void) -> HWND {
    let class = wstr(cls);
    let s = st();
    let Some(k) = s.classes.iter().find(|k| k.name == class) else { return 0 };
    let (proc_, class_style, class_cursor) = (k.proc_, k.style, k.cursor);
    let role = role_for(&class, ex);
    let xs = x();
    let c = &xs.conn;
    let Ok(id) = c.generate_id() else { return 0 };
    let (w, h) = (w0.max(1), h0.max(1));
    if role == Role::Host {
        let aux = CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE);
        if c.create_window(0, id, xs.root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &aux).is_err() {
            return 0;
        }
    } else {
        let mask = EventMask::EXPOSURE
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW
            | EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE
            | EventMask::FOCUS_CHANGE
            | EventMask::PROPERTY_CHANGE;
        let aux = CreateWindowAux::new()
            .background_pixel(0)
            .border_pixel(0)
            .colormap(xs.colormap)
            .override_redirect(u32::from(role == Role::Ghost))
            .event_mask(mask);
        if c.create_window(32, id, xs.root, x0 as i16, y0 as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, xs.argb, &aux).is_err() {
            return 0;
        }
        let title = wstring(name);
        setup_props(xs, id, role, ex, &title, x0, y0, w, h);
        if role == Role::Ghost && xs.has_shape {
            let _ = c.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, id, 0, 0, &[]);
        }
    }
    let gc = if role == Role::Host {
        0
    } else {
        let g = c.generate_id().unwrap_or(0);
        let _ = c.create_gc(g, id, &CreateGCAux::new().graphics_exposures(0));
        g
    };
    s.wins.insert(id, Win {
        id,
        proc_,
        class_style,
        class_cursor,
        role,
        ex,
        user: 0,
        x: x0,
        y: y0,
        w,
        h,
        visible: false,
        mapped: false,
        track_leave: false,
        inside: false,
        gc,
        pixmap: 0,
        pw: 0,
        ph: 0,
        shape_hash: 0,
        want_focus: false,
        dnd: false,
        layer: None,
    });
    id as HWND
}

unsafe fn setup_props(xs: &X, id: u32, role: Role, ex: u32, title: &str, x0: i32, y0: i32, w: i32, h: i32) {
    let c = &xs.conn;
    let a = &xs.atoms;
    let _ = c.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, b"crowfo\0CrowFO\0");
    let t = if title.is_empty() { "Crow FO" } else { title };
    let _ = c.change_property8(PropMode::REPLACE, id, a._NET_WM_NAME, a.UTF8_STRING, t.as_bytes());
    let _ = c.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, t.as_bytes());
    let _ = c.change_property32(PropMode::REPLACE, id, a._NET_WM_PID, AtomEnum::CARDINAL, &[std::process::id()]);
    let _ = c.change_property32(PropMode::REPLACE, id, a.WM_PROTOCOLS, AtomEnum::ATOM, &[a.WM_DELETE_WINDOW]);
    // No frame: Crow draws its own rounded windows.
    let _ = c.change_property32(PropMode::REPLACE, id, a._MOTIF_WM_HINTS, a._MOTIF_WM_HINTS, &[2, 0, 0, 0, 0]);
    // ICCCM hints: input focus only for windows that can be typed into, and
    // exact placement from us.
    let input = u32::from(role == Role::App || role == Role::Popup);
    let _ = c.change_property32(PropMode::REPLACE, id, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, &[1, input, 1, 0, 0, 0, 0, 0, 0]);
    let mut hints = [0u32; 18];
    hints[0] = 1 | 2 | 4 | 8 | 512; // USPosition, USSize, PPosition, PSize, PWinGravity
    hints[1] = x0 as u32;
    hints[2] = y0 as u32;
    hints[3] = w as u32;
    hints[4] = h as u32;
    hints[17] = 1; // NorthWest
    let _ = c.change_property32(PropMode::REPLACE, id, AtomEnum::WM_NORMAL_HINTS, AtomEnum::WM_SIZE_HINTS, &hints);
    if role == Role::App {
        if let Some(icon) = app_icon() {
            let _ = c.change_property32(PropMode::REPLACE, id, a._NET_WM_ICON, AtomEnum::CARDINAL, &icon);
        }
    }
    let ty = match role {
        Role::Desk => a._NET_WM_WINDOW_TYPE_DOCK,
        Role::Popup => a._NET_WM_WINDOW_TYPE_UTILITY,
        Role::Ghost => a._NET_WM_WINDOW_TYPE_DND,
        _ => a._NET_WM_WINDOW_TYPE_NORMAL,
    };
    let _ = c.change_property32(PropMode::REPLACE, id, a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, &[ty]);
    let mut state = Vec::new();
    if role != Role::App {
        state.extend([a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER, a._NET_WM_STATE_STICKY]);
        let _ = c.change_property32(PropMode::REPLACE, id, a._NET_WM_DESKTOP, AtomEnum::CARDINAL, &[0xFFFF_FFFF]);
    }
    if ex & WS_EX_TOPMOST != 0 || role == Role::Popup {
        state.push(a._NET_WM_STATE_ABOVE);
    } else if role == Role::Desk {
        state.push(a._NET_WM_STATE_BELOW);
    }
    let _ = c.change_property32(PropMode::REPLACE, id, a._NET_WM_STATE, AtomEnum::ATOM, &state);
}

/// The Crow logo for the taskbar, as _NET_WM_ICON data.
fn app_icon() -> Option<Vec<u32>> {
    let mut v = Vec::new();
    for s in [32, 64] {
        let px = crate::art::logo(s);
        v.push(s as u32);
        v.push(s as u32);
        // straight alpha, which _NET_WM_ICON wants
        v.extend(px.iter().map(|&c| unpremul(c)));
    }
    Some(v)
}

fn unpremul(c: u32) -> u32 {
    let a = c >> 24;
    if a == 0 || a == 255 {
        return c;
    }
    let f = |v: u32| ((v * 255 + a / 2) / a).min(255);
    (a << 24) | (f((c >> 16) & 255) << 16) | (f((c >> 8) & 255) << 8) | f(c & 255)
}

pub unsafe extern "system" fn DestroyWindow(h: HWND) -> BOOL {
    let s = st();
    let Some(wd) = s.wins.remove(&(h as u32)) else { return 0 };
    s.timers.retain(|t| t.hwnd != h);
    s.queue.retain(|m| m.hwnd != h);
    let xs = x();
    if s.capture == h {
        s.capture = 0;
        let _ = xs.conn.ungrab_pointer(x11rb::CURRENT_TIME);
    }
    if s.hover == h {
        s.hover = 0;
    }
    if s.focus == h {
        s.focus = 0;
    }
    if wd.gc != 0 {
        let _ = xs.conn.free_gc(wd.gc);
    }
    if wd.pixmap != 0 {
        let _ = xs.conn.free_pixmap(wd.pixmap);
    }
    let _ = xs.conn.destroy_window(wd.id);
    xs.flush();
    1
}

pub unsafe extern "system" fn IsWindow(h: HWND) -> BOOL {
    is_ours(h) as BOOL
}

pub unsafe extern "system" fn IsWindowVisible(h: HWND) -> BOOL {
    win(h).is_some_and(|w| w.visible) as BOOL
}

pub unsafe extern "system" fn ShowWindow(h: HWND, cmd: i32) -> BOOL {
    let Some(wd) = win(h) else { return 0 };
    let was = wd.visible;
    if wd.role == Role::Host {
        return was as BOOL;
    }
    let xs = x();
    if cmd == SW_HIDE {
        if wd.visible {
            let _ = xs.conn.unmap_window(wd.id);
            wd.visible = false;
            wd.mapped = false;
        }
    } else if !wd.visible {
        // Placement again right before mapping, some window managers only
        // read it then.
        let _ = xs.conn.configure_window(wd.id, &ConfigureWindowAux::new().x(wd.x).y(wd.y).width(wd.w as u32).height(wd.h as u32));
        let _ = xs.conn.map_window(wd.id);
        wd.visible = true;
    }
    xs.flush();
    was as BOOL
}

fn set_state(id: u32, add: bool, a1: u32, a2: u32) {
    let xs = x();
    let wd = win(id as HWND);
    if wd.as_ref().is_some_and(|w| w.visible) {
        xs.wm_message(id, xs.atoms._NET_WM_STATE, [u32::from(add), a1, a2, 2, 0]);
    } else {
        // Not mapped yet: edit the property the window manager reads on map.
        let mut cur = xs.prop32(id, xs.atoms._NET_WM_STATE, AtomEnum::ATOM);
        cur.retain(|&v| v != a1 && (a2 == 0 || v != a2));
        if add {
            cur.push(a1);
            if a2 != 0 {
                cur.push(a2);
            }
        }
        let _ = xs.conn.change_property32(PropMode::REPLACE, id, xs.atoms._NET_WM_STATE, AtomEnum::ATOM, &cur);
    }
}

/// Puts a window in the "above" band, the normal band, or the "below" band
/// (folders resting on the desktop).
pub fn set_layer(h: HWND, layer: i32) {
    let Some(wd) = win(h) else { return };
    if wd.role == Role::Host || wd.role == Role::Ghost {
        return;
    }
    // Nothing to tell the window manager when it is already there: every
    // request makes it restack and announce changes, which come back to us.
    if wd.layer == Some(layer) {
        return;
    }
    wd.layer = Some(layer);
    let a = &x().atoms;
    let (above, below) = (a._NET_WM_STATE_ABOVE, a._NET_WM_STATE_BELOW);
    match layer {
        1 => {
            wd.ex |= WS_EX_TOPMOST;
            set_state(wd.id, false, below, 0);
            set_state(wd.id, true, above, 0);
        }
        -1 => {
            wd.ex &= !WS_EX_TOPMOST;
            set_state(wd.id, false, above, 0);
            set_state(wd.id, true, below, 0);
        }
        _ => {
            wd.ex &= !WS_EX_TOPMOST;
            set_state(wd.id, false, above, below);
        }
    }
    let _ = x().conn.configure_window(wd.id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    x().flush();
}

pub unsafe extern "system" fn SetWindowPos(h: HWND, after: HWND, x0: i32, y0: i32, cx: i32, cy: i32, f: u32) -> BOOL {
    let Some(wd) = win(h) else { return 0 };
    let mut aux = ConfigureWindowAux::new();
    if f & SWP_NOMOVE == 0 {
        wd.x = x0;
        wd.y = y0;
        aux = aux.x(x0).y(y0);
    }
    if f & SWP_NOSIZE == 0 {
        wd.w = cx.max(1);
        wd.h = cy.max(1);
        aux = aux.width(wd.w as u32).height(wd.h as u32);
    }
    let xs = x();
    if wd.role != Role::Host {
        let _ = xs.conn.configure_window(wd.id, &aux);
    }
    if f & SWP_NOZORDER == 0 {
        match after {
            HWND_TOPMOST => set_layer(h, 1),
            HWND_NOTOPMOST => {
                if wd.ex & WS_EX_TOPMOST != 0 {
                    set_layer(h, 0)
                }
            }
            HWND_BOTTOM => {
                let _ = xs.conn.configure_window(wd.id, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW));
            }
            _ => {
                let _ = xs.conn.configure_window(wd.id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
            }
        }
    }
    if f & SWP_SHOWWINDOW != 0 {
        ShowWindow(h, SW_SHOWNOACTIVATE);
    }
    xs.flush();
    1
}

pub unsafe extern "system" fn GetWindowRect(h: HWND, r: *mut RECT) -> BOOL {
    let Some(wd) = win(h) else { return 0 };
    *r = RECT { left: wd.x, top: wd.y, right: wd.x + wd.w, bottom: wd.y + wd.h };
    1
}

pub unsafe extern "system" fn GetWindowLongPtrW(h: HWND, i: i32) -> isize {
    let Some(wd) = win(h) else { return 0 };
    match i {
        GWL_EXSTYLE => wd.ex as isize,
        GWLP_USERDATA => wd.user,
        _ => 0,
    }
}

pub unsafe extern "system" fn SetWindowLongPtrW(h: HWND, i: i32, v: isize) -> isize {
    let Some(wd) = win(h) else { return 0 };
    match i {
        GWL_EXSTYLE => {
            let old = wd.ex;
            wd.ex = v as u32;
            if (old ^ wd.ex) & WS_EX_NOACTIVATE != 0 && wd.role != Role::Host {
                // a folder being renamed takes keyboard input for a while
                let input = u32::from(wd.ex & WS_EX_NOACTIVATE == 0);
                let _ = x().conn.change_property32(PropMode::REPLACE, wd.id, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, &[1, input, 1, 0, 0, 0, 0, 0, 0]);
            }
            old as isize
        }
        GWLP_USERDATA => core::mem::replace(&mut wd.user, v),
        _ => 0,
    }
}

pub unsafe extern "system" fn SetWindowTextW(h: HWND, s: PCWSTR) -> BOOL {
    let Some(wd) = win(h) else { return 0 };
    let t = wstring(s);
    let xs = x();
    let _ = xs.conn.change_property8(PropMode::REPLACE, wd.id, xs.atoms._NET_WM_NAME, xs.atoms.UTF8_STRING, t.as_bytes());
    1
}

// ------------------------------------------------------------------ pixels
pub unsafe extern "system" fn UpdateLayeredWindow(h: HWND, _dst: HDC, pos: *const POINT, sz: *const SIZE, src: HDC, spt: *const POINT,
    _key: u32, blend: *const BLENDFUNCTION, _f: u32) -> BOOL {
    let Some(wd) = win(h) else { return 0 };
    if wd.role == Role::Host {
        return 0;
    }
    let xs = x();
    let (nx, ny) = if pos.is_null() { (wd.x, wd.y) } else { ((*pos).x, (*pos).y) };
    let (nw, nh) = if sz.is_null() { (wd.w, wd.h) } else { ((*sz).cx.max(1), (*sz).cy.max(1)) };
    if (nx, ny, nw, nh) != (wd.x, wd.y, wd.w, wd.h) {
        let mut aux = ConfigureWindowAux::new();
        if (nx, ny) != (wd.x, wd.y) {
            aux = aux.x(nx).y(ny);
        }
        if (nw, nh) != (wd.w, wd.h) {
            aux = aux.width(nw as u32).height(nh as u32);
        }
        let _ = xs.conn.configure_window(wd.id, &aux);
        (wd.x, wd.y, wd.w, wd.h) = (nx, ny, nw, nh);
    }
    let Some((bits, bw, bh)) = gdi::dc_pixels(src) else { return 0 };
    let (sx, sy) = if spt.is_null() { (0, 0) } else { ((*spt).x, (*spt).y) };
    let k = if blend.is_null() { 255 } else { (*blend).SourceConstantAlpha as u32 };
    // Usually the whole bitmap at full opacity, sent as is. Otherwise a
    // temporary copy with the requested part and opacity.
    let direct = k == 255 && sx == 0 && sy == 0 && bw == nw && bh >= nh;
    let tmp: Vec<u32>;
    let px: &[u32] = if direct {
        core::slice::from_raw_parts(bits, (nw * nh) as usize)
    } else {
        let mut v = Vec::with_capacity((nw * nh) as usize);
        for y in 0..nh {
            let yy = y + sy;
            for xx in 0..nw {
                let xs_ = xx + sx;
                let c = if yy >= 0 && yy < bh && xs_ >= 0 && xs_ < bw { *bits.add((yy * bw + xs_) as usize) } else { 0 };
                v.push(if k == 255 { c } else { crate::gfx::scale(c, k + (k >> 7)) });
            }
        }
        tmp = v;
        &tmp
    };
    if (wd.pw, wd.ph) != (nw, nh) || wd.pixmap == 0 {
        if wd.pixmap != 0 {
            let _ = xs.conn.free_pixmap(wd.pixmap);
        }
        let pm = xs.conn.generate_id().unwrap_or(0);
        let _ = xs.conn.create_pixmap(32, pm, wd.id, nw as u16, nh as u16);
        let _ = xs.conn.change_window_attributes(wd.id, &ChangeWindowAttributesAux::new().background_pixmap(pm));
        wd.pixmap = pm;
        wd.pw = nw;
        wd.ph = nh;
    }
    upload(wd, px);
    let _ = xs.conn.clear_area(false, wd.id, 0, 0, 0, 0);
    if wd.role != Role::Ghost {
        update_shape(wd, px);
    }
    xs.flush();
    1
}

/// Sends an image to the window's pixmap, in slices that fit a request.
fn upload(wd: &Win, px: &[u32]) {
    if wd.gc == 0 || wd.pixmap == 0 {
        return;
    }
    let xs = x();
    let row = wd.pw as usize * 4;
    let max_rows = ((xs.max_request.min(4 << 20) - 64) / row).max(1);
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(px.as_ptr() as *const u8, px.len() * 4) };
    let mut y = 0usize;
    while y < wd.ph as usize {
        let n = max_rows.min(wd.ph as usize - y);
        let _ = xs.conn.put_image(ImageFormat::Z_PIXMAP, wd.pixmap, wd.gc, wd.pw as u16, n as u16, 0, y as i16, 0, 32, &bytes[y * row..(y + n) * row]);
        y += n;
    }
}

/// Clicks reach the window only where it isn't fully transparent.
fn update_shape(wd: &mut Win, px: &[u32]) {
    let xs = x();
    if !xs.has_shape {
        return;
    }
    let (w, h) = (wd.pw as usize, wd.ph as usize);
    let mut rects: Vec<Rectangle> = Vec::new();
    let mut prev: Vec<(u16, u16)> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ (w as u64) << 32 ^ h as u64;
    for y in 0..h {
        let line = &px[y * w..(y + 1) * w];
        let mut runs: Vec<(u16, u16)> = Vec::new();
        let mut x0 = None;
        for (x1, &c) in line.iter().enumerate() {
            match (c >> 24 != 0, x0) {
                (true, None) => x0 = Some(x1),
                (false, Some(s)) => {
                    runs.push((s as u16, x1 as u16));
                    x0 = None;
                }
                _ => {}
            }
        }
        if let Some(s) = x0 {
            runs.push((s as u16, w as u16));
        }
        if runs == prev {
            for &i in &open {
                rects[i].height += 1;
            }
        } else {
            open.clear();
            for &(a, b) in &runs {
                open.push(rects.len());
                rects.push(Rectangle { x: a as i16, y: y as i16, width: b - a, height: 1 });
            }
            prev = runs;
        }
        for &(a, b) in &prev {
            hash = (hash ^ ((a as u64) << 16 | b as u64)).wrapping_mul(0x100_0000_01b3);
        }
        hash = (hash ^ 0xff).wrapping_mul(0x100_0000_01b3);
    }
    if hash == wd.shape_hash {
        return;
    }
    wd.shape_hash = hash;
    let _ = xs.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, wd.id, 0, 0, &rects);
}

// ------------------------------------------------------------------ messages
pub unsafe extern "system" fn PostMessageW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> BOOL {
    st().queue.push_back(MSG { hwnd: h, message: m, wParam: w, lParam: l, time: 0, pt: POINT::default(), priv_: 0 });
    1
}

pub unsafe extern "system" fn SendMessageW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    call(h, m, w, l)
}

pub unsafe extern "system" fn PostQuitMessage(_code: i32) {
    st().quit = true;
}

/// Sends a message to every window of the hidden host class.
pub fn broadcast_host(m: u32, w: WPARAM, l: LPARAM) {
    let hosts: Vec<HWND> = st().wins.values().filter(|w| w.role == Role::Host).map(|w| w.id as HWND).collect();
    for h in hosts {
        unsafe { call(h, m, w, l) };
    }
}

// --------------------------------------------------------------------- timers
pub unsafe extern "system" fn SetTimer(h: HWND, id: usize, ms: u32, _f: usize) -> usize {
    let s = st();
    let due = now() + ms.max(1) as f64;
    match s.timers.iter_mut().find(|t| t.hwnd == h && t.id == id) {
        Some(t) => {
            t.ms = ms.max(1);
            t.due = due;
        }
        None => s.timers.push(Timer { hwnd: h, id, ms: ms.max(1), due }),
    }
    id
}

pub unsafe extern "system" fn KillTimer(h: HWND, id: usize) -> BOOL {
    let s = st();
    let n = s.timers.len();
    s.timers.retain(|t| !(t.hwnd == h && t.id == id));
    (s.timers.len() != n) as BOOL
}

/// Milliseconds until the next timer, if any.
pub fn next_timer_ms() -> Option<u64> {
    let t = now();
    st().timers.iter().map(|k| (k.due - t).max(0.0) as u64).min()
}

fn fire_timers() {
    let t = now();
    let due: Vec<(HWND, usize)> = st().timers.iter().filter(|k| k.due <= t).map(|k| (k.hwnd, k.id)).collect();
    for (h, id) in due {
        // an earlier timer may have killed or reset this one
        let Some(k) = st().timers.iter_mut().find(|k| k.hwnd == h && k.id == id) else { continue };
        if k.due > t {
            continue;
        }
        k.due = t + k.ms as f64;
        unsafe { call(h, WM_TIMER, id, 0) };
    }
}

// ------------------------------------------------------------------- the loop
/// Handles everything that is ready: X events, due timers and posted
/// messages. Returns false once the app asked to quit.
pub fn pump() -> bool {
    let xs = x();
    let mut evs: Vec<Event> = Vec::new();
    while let Some(e) = xs.poll() {
        evs.push(e);
    }
    // Only the newest pointer position matters.
    let n = evs.len();
    for i in 0..n {
        let skip = matches!(&evs[i], Event::MotionNotify(a) if matches!(evs.get(i + 1), Some(Event::MotionNotify(b)) if b.event == a.event));
        if !skip {
            unsafe { handle(&evs[i]) };
        }
    }
    fire_timers();
    while let Some(m) = st().queue.pop_front() {
        if m.message == WM_QUIT {
            st().quit = true;
            break;
        }
        if st().busy_since > 0.0 {
            note(format!("msg {:#x}", m.message));
        }
        unsafe { call(m.hwnd, m.message, m.wParam, m.lParam) };
    }
    xs.flush();
    !st().quit && !xs.broken
}

/// Frees the keyboard map and the monitor list (read again when needed).
pub fn release_caches() {
    let s = st();
    s.keymap = Vec::new();
    s.monitors = None;
}

pub fn quit_requested() -> bool {
    st().quit
}

/// Whether input is waiting (so a frame wait can stop early).
pub fn has_pending() -> bool {
    !st().queue.is_empty() || !x().pending.is_empty()
}

unsafe fn handle(e: &Event) {
    if st().busy_since > 0.0 {
        note(event_name(e));
    }
    let s = st();
    match e {
        Event::ConfigureNotify(ev) => {
            let h = ev.window as HWND;
            let Some(wd) = win(h) else { return };
            // Real positions come from synthetic events (sent by the window
            // manager) or from windows it doesn't manage.
            let moved = if ev.response_type & 0x80 != 0 || wd.role == Role::Ghost {
                let (ox, oy) = (wd.x, wd.y);
                wd.x = ev.x as i32;
                wd.y = ev.y as i32;
                (ox, oy) != (wd.x, wd.y)
            } else {
                false
            };
            if moved {
                call(h, WM_MOVE, 0, pack(wd.x, wd.y));
            }
        }
        Event::MapNotify(ev) => {
            if let Some(wd) = win(ev.window as HWND) {
                wd.mapped = true;
                if wd.want_focus {
                    wd.want_focus = false;
                    let _ = x().conn.set_input_focus(InputFocus::PARENT, wd.id, x11rb::CURRENT_TIME);
                }
            }
        }
        Event::UnmapNotify(ev) => {
            if let Some(wd) = win(ev.window as HWND) {
                wd.mapped = false;
            }
        }
        Event::EnterNotify(ev) => {
            let h = ev.event as HWND;
            if let Some(wd) = win(h) {
                wd.inside = true;
                s.hover = h;
                set_cursor_for(h, ev.root_x as i32, ev.root_y as i32);
            }
        }
        Event::LeaveNotify(ev) => {
            let h = ev.event as HWND;
            if ev.detail == NotifyDetail::INFERIOR || ev.mode == NotifyMode::GRAB {
                return;
            }
            if let Some(wd) = win(h) {
                wd.inside = false;
                if s.hover == h {
                    s.hover = 0;
                }
                // Windows holds WM_MOUSELEAVE back while a window has the capture.
                if wd.track_leave && s.capture != h {
                    wd.track_leave = false;
                    call(h, WM_MOUSELEAVE, 0, 0);
                }
            }
        }
        Event::MotionNotify(ev) => {
            let h = ev.event as HWND;
            if let Some(wd) = win(h) {
                if !wd.inside && s.capture != h {
                    wd.inside = true;
                    s.hover = h;
                }
                set_cursor_for(h, ev.root_x as i32, ev.root_y as i32);
                call(h, WM_MOUSEMOVE, mk(ev.state), pack(ev.event_x as i32, ev.event_y as i32));
            }
        }
        Event::ButtonPress(ev) => {
            let h = ev.event as HWND;
            let Some(wd) = win(h) else { return };
            let (cx, cy) = (ev.event_x as i32, ev.event_y as i32);
            let l = pack(cx, cy);
            match ev.detail {
                1 => {
                    let ht = call(h, WM_NCHITTEST, 0, pack(ev.root_x as i32, ev.root_y as i32));
                    if ht == HTCAPTION {
                        start_move(wd, ev.root_x as i32, ev.root_y as i32);
                        return;
                    }
                    let t = now();
                    let (lh, lt, lx, ly) = s.last_click;
                    let dbl = wd.class_style & CS_DBLCLKS != 0 && lh == h && t - lt < 400.0 && (cx - lx).abs() <= 4 && (cy - ly).abs() <= 4;
                    s.last_click = if dbl { (0, 0.0, 0, 0) } else { (h, t, cx, cy) };
                    call(h, if dbl { WM_LBUTTONDBLCLK } else { WM_LBUTTONDOWN }, mk(ev.state) | MK_LBUTTON, l);
                }
                2 => {
                    call(h, 0x207, mk(ev.state), l);
                }
                3 => {
                    call(h, WM_RBUTTONDOWN, mk(ev.state) | 2, l);
                }
                4 | 5 | 6 | 7 => {
                    let d: i16 = if ev.detail == 4 || ev.detail == 7 { 120 } else { -120 };
                    let m = if ev.detail >= 6 { WM_MOUSEHWHEEL } else { WM_MOUSEWHEEL };
                    call(h, m, ((d as u16 as usize) << 16) | mk(ev.state), pack(ev.root_x as i32, ev.root_y as i32));
                }
                _ => {}
            }
        }
        Event::ButtonRelease(ev) => {
            let h = ev.event as HWND;
            if win(h).is_none() {
                return;
            }
            let l = pack(ev.event_x as i32, ev.event_y as i32);
            match ev.detail {
                1 => {
                    call(h, WM_LBUTTONUP, mk(ev.state) & !MK_LBUTTON, l);
                }
                2 => {
                    call(h, WM_MBUTTONUP, mk(ev.state), l);
                }
                3 => {
                    call(h, WM_RBUTTONUP, mk(ev.state), l);
                }
                _ => {}
            }
        }
        Event::KeyPress(ev) => key(ev),
        Event::FocusIn(ev) => {
            if ev.mode == NotifyMode::GRAB || ev.mode == NotifyMode::UNGRAB || ev.detail == NotifyDetail::POINTER {
                return;
            }
            let h = ev.event as HWND;
            if is_ours(h) && s.focus != h {
                s.focus = h;
                call(h, WM_SETFOCUS, 0, 0);
                call(h, WM_ACTIVATE, 1, 0);
            }
        }
        Event::FocusOut(ev) => {
            if ev.mode == NotifyMode::GRAB || ev.mode == NotifyMode::UNGRAB || ev.detail == NotifyDetail::POINTER || ev.detail == NotifyDetail::INFERIOR {
                return;
            }
            let h = ev.event as HWND;
            if s.focus == h {
                s.focus = 0;
                s.dead = None;
                call(h, WM_KILLFOCUS, 0, 0);
                call(h, WM_ACTIVATE, WA_INACTIVE, 0);
            }
        }
        Event::PropertyNotify(ev) if ev.window == x().root => {
            let a = &x().atoms;
            if ev.atom == a._NET_ACTIVE_WINDOW || ev.atom == a._NET_SHOWING_DESKTOP {
                foreground_changed();
            } else if ev.atom == a._NET_WORKAREA {
                // window managers repeat it on many changes; only a real one counts
                let now = workarea();
                if now != s.last_workarea {
                    s.last_workarea = now;
                    s.monitors = None;
                    broadcast_host(WM_SETTINGCHANGE, 0x2F, 0);
                }
            }
        }
        Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => {
            s.monitors = None;
            broadcast_host(WM_DISPLAYCHANGE, 0, 0);
        }
        Event::ClientMessage(ev) => {
            let a = &x().atoms;
            if ev.type_ == a.WM_PROTOCOLS && ev.data.as_data32()[0] == a.WM_DELETE_WINDOW {
                call(ev.window as HWND, WM_CLOSE, 0, 0);
            } else if let Some(f) = s.client_hook {
                f(e);
            }
        }
        Event::SelectionRequest(_) | Event::SelectionClear(_) => {
            if !shell::clipboard_event(e) {
                if let Some(f) = s.client_hook {
                    f(e);
                }
            }
        }
        Event::SelectionNotify(_) => {
            if let Some(f) = s.client_hook {
                f(e);
            }
        }
        Event::MappingNotify(_) => {
            s.keymap.clear();
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ watchdog
fn note(what: String) {
    let s = st();
    if s.busy_since > 0.0 {
        *s.seen.entry(what).or_insert(0) += 1;
    }
}

fn event_name(e: &Event) -> String {
    let d = format!("{e:?}");
    let name = d.split(['(', ' ', '{']).next().unwrap_or("?").to_string();
    if let Event::PropertyNotify(p) = e {
        let n = x().conn.get_atom_name(p.atom).ok().and_then(|c| c.reply().ok()).map(|r| String::from_utf8_lossy(&r.name).into_owned()).unwrap_or_default();
        return format!("{name} {n}");
    }
    name
}

/// Called each time the main loop is about to wait. When it hasn't really
/// slept for two seconds, Crow is spinning: what it was handling goes to
/// ~/.cache/crowfo/crow.log so the cause can be found.
pub fn watchdog(sleeping: bool) {
    let s = st();
    let t = now();
    if sleeping {
        s.busy_since = 0.0;
        s.seen.clear();
        return;
    }
    if s.busy_since == 0.0 {
        s.busy_since = t;
        return;
    }
    if t - s.busy_since < 2000.0 {
        return;
    }
    let mut v: Vec<(&String, &u32)> = s.seen.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1));
    let line: Vec<String> = v.iter().take(12).map(|(k, n)| format!("{k} x{n}")).collect();
    let dir = crate::ffi::lin_path(&crate::store::cache_dir());
    let log = dir.join("crow.log");
    let old = std::fs::read_to_string(&log).unwrap_or_default();
    let keep: String = old.lines().rev().take(200).collect::<Vec<_>>().into_iter().rev().map(|l| format!("{l}\n")).collect();
    let stamp = std::process::Command::new("date").arg("+%F %T").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let _ = std::fs::write(&log, format!("{keep}{stamp} busy for 2 s, v{}: {}\n", env!("CROW_VERSION"), line.join(", ")));
    s.busy_since = t;
    s.seen.clear();
}

fn mk(state: KeyButMask) -> usize {
    let mut v = 0;
    if state.contains(KeyButMask::BUTTON1) {
        v |= MK_LBUTTON;
    }
    if state.contains(KeyButMask::BUTTON3) {
        v |= 2;
    }
    if state.contains(KeyButMask::SHIFT) {
        v |= 4;
    }
    if state.contains(KeyButMask::CONTROL) {
        v |= 8;
    }
    v
}

/// The title bar of the settings window moves it, like a native one.
unsafe fn start_move(wd: &Win, rx: i32, ry: i32) {
    let xs = x();
    let _ = xs.conn.ungrab_pointer(x11rb::CURRENT_TIME);
    xs.wm_message(wd.id, xs.atoms._NET_WM_MOVERESIZE, [rx as u32, ry as u32, 8, 1, 1]);
    xs.flush();
}

fn foreground_changed() {
    let xs = x();
    let a = &xs.atoms;
    let act = xs.prop32(xs.root, a._NET_ACTIVE_WINDOW, AtomEnum::WINDOW).first().copied().unwrap_or(0) as HWND;
    let showing = xs.prop32(xs.root, a._NET_SHOWING_DESKTOP, AtomEnum::CARDINAL).first().is_some_and(|&v| v != 0);
    let s = st();
    if act == s.active && showing == s.showing_desktop {
        return;
    }
    s.active = act;
    s.showing_desktop = showing;
    if is_ours(act) {
        return;
    }
    if let Some(f) = s.fg_hook {
        unsafe { f(0, EVENT_SYSTEM_FOREGROUND, if showing { -1 } else { act }, 0, 0, 0, 0) };
    }
}

/// Whether the desktop is showing (Super+D), as told by the window manager.
pub fn showing_desktop() -> bool {
    st().showing_desktop
}

// ------------------------------------------------------------------ keyboard
fn keymap() {
    let s = st();
    if !s.keymap.is_empty() {
        return;
    }
    let xs = x();
    let setup = xs.conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    if let Ok(Ok(r)) = xs.conn.get_keyboard_mapping(min, max - min + 1).map(|c| c.reply()) {
        s.per = r.keysyms_per_keycode as usize;
        s.keymap = r.keysyms;
        s.min_kc = min;
    }
}

fn keysym(code: u8, col: usize) -> u32 {
    keymap();
    let s = st();
    if s.per == 0 || code < s.min_kc {
        return 0;
    }
    let base = (code - s.min_kc) as usize * s.per;
    let get = |c: usize| if c < s.per { s.keymap.get(base + c).copied().unwrap_or(0) } else { 0 };
    let v = get(col);
    if v == 0 && col % 2 == 1 {
        // no shifted symbol: the shifted letter is the upper case one
        let b = get(col - 1);
        if (0x61..=0x7a).contains(&b) {
            return b - 0x20;
        }
        return b;
    }
    v
}

/// The keycode of a keysym, for GetAsyncKeyState.
fn keycode(sym: u32) -> Option<u8> {
    keymap();
    let s = st();
    let i = s.keymap.iter().position(|&k| k == sym)?;
    Some(s.min_kc + (i / s.per.max(1)) as u8)
}

unsafe fn key(ev: &KeyPressEvent) {
    let s = st();
    let h = if is_ours(ev.event as HWND) { ev.event as HWND } else { s.focus };
    if !is_ours(h) {
        return;
    }
    let st_ = u16::from(ev.state);
    let shift = st_ & 1 != 0;
    let caps = st_ & 2 != 0;
    let ctrl = st_ & 4 != 0;
    let alt = st_ & 8 != 0;
    let level3 = st_ & 0x80 != 0 || st_ & 0x2000 != 0; // Mod5 (AltGr) or group 2
    let base = if level3 { 4 } else { 0 };
    let mut sym = keysym(ev.detail, base + usize::from(shift));
    if sym == 0 && level3 {
        sym = keysym(ev.detail, usize::from(shift));
    }
    if caps && !shift {
        if let Some(c) = keys::char_of(sym) {
            if c.is_lowercase() {
                let up = keysym(ev.detail, base + 1);
                if up != 0 {
                    sym = up;
                }
            }
        }
    }
    let unshifted = keysym(ev.detail, 0);
    let vk = match keys::vk(unshifted) {
        0 => keys::vk(sym),
        v => v,
    };
    if vk != 0 {
        call(h, if alt { WM_SYSKEYDOWN } else { WM_KEYDOWN }, vk, 1);
    }
    if !is_ours(h) {
        return;
    }
    if let Some(m) = keys::dead(sym) {
        s.dead = Some(m);
        return;
    }
    // control characters, like TranslateMessage makes them
    let ch: Option<u32> = match vk {
        0x08 => Some(8),
        0x09 => Some(9),
        0x0D => Some(13),
        0x1B => Some(27),
        _ if ctrl && !level3 => match keys::char_of(unshifted) {
            Some(c) if c.is_ascii_alphabetic() => Some((c.to_ascii_uppercase() as u32) & 0x1F),
            _ => None,
        },
        _ if alt && !level3 => None,
        _ => keys::char_of(sym).map(|c| {
            let c = match s.dead.take() {
                Some(m) => keys::compose(m, c).unwrap_or(c),
                None => c,
            };
            c as u32
        }),
    };
    if let Some(c) = ch {
        let mut buf = [0u16; 2];
        for &u in char::from_u32(c).unwrap_or('\u{fffd}').encode_utf16(&mut buf).iter() {
            call(h, WM_CHAR, u as usize, 1);
        }
    }
}

pub unsafe extern "system" fn GetKeyState(k: i32) -> i16 {
    let xs = x();
    let Ok(Ok(r)) = xs.conn.query_pointer(xs.root).map(|c| c.reply()) else { return 0 };
    let m = u16::from(r.mask);
    let down = match k {
        0x01 => m & 0x100 != 0,
        0x02 => m & 0x400 != 0,
        0x10 => m & 1 != 0,
        0x11 => m & 4 != 0,
        0x12 => m & 8 != 0,
        _ => return async_key(k),
    };
    if down { -128 } else { 0 }
}

unsafe fn async_key(k: i32) -> i16 {
    let sym = match k {
        0x1B => 0xff1b,
        0x0D => 0xff0d,
        _ => return 0,
    };
    let Some(code) = keycode(sym) else { return 0 };
    let xs = x();
    let Ok(Ok(r)) = xs.conn.query_keymap().map(|c| c.reply()) else { return 0 };
    if r.keys[(code / 8) as usize] & (1 << (code % 8)) != 0 { -32768 } else { 0 }
}

pub unsafe extern "system" fn GetAsyncKeyState(k: i32) -> i16 {
    match GetKeyState(k) {
        0 => 0,
        _ => -32768,
    }
}

// --------------------------------------------------------- focus and capture
pub unsafe extern "system" fn SetCapture(h: HWND) -> HWND {
    let s = st();
    let old = s.capture;
    let Some(wd) = win(h) else { return old };
    let xs = x();
    let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION | EventMask::ENTER_WINDOW | EventMask::LEAVE_WINDOW;
    let _ = xs.conn.grab_pointer(false, wd.id, mask, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME);
    s.capture = h;
    xs.flush();
    old
}

pub unsafe extern "system" fn ReleaseCapture() -> BOOL {
    let s = st();
    let old = s.capture;
    s.capture = 0;
    let xs = x();
    let _ = xs.conn.ungrab_pointer(x11rb::CURRENT_TIME);
    xs.flush();
    if old != 0 {
        call(old, WM_CAPTURECHANGED, 0, 0);
        // a leave that happened during the capture shows up now
        if let Some(wd) = win(old) {
            let mut p = POINT::default();
            GetCursorPos(&mut p);
            let inside = p.x >= wd.x && p.x < wd.x + wd.w && p.y >= wd.y && p.y < wd.y + wd.h && window_at(p) == old;
            if !inside && wd.track_leave {
                wd.track_leave = false;
                wd.inside = false;
                call(old, WM_MOUSELEAVE, 0, 0);
            }
        }
    }
    1
}

pub unsafe extern "system" fn GetCapture() -> HWND {
    st().capture
}

pub unsafe extern "system" fn TrackMouseEvent(t: *mut TRACKMOUSEEVENT) -> BOOL {
    let t = &*t;
    let Some(wd) = win(t.hwndTrack) else { return 0 };
    if t.dwFlags & TME_LEAVE != 0 {
        wd.track_leave = true;
        if !wd.inside && st().capture != t.hwndTrack {
            PostMessageW(t.hwndTrack, WM_MOUSELEAVE, 0, 0);
            wd.track_leave = false;
        }
    }
    1
}

pub unsafe extern "system" fn SetFocus(h: HWND) -> HWND {
    let s = st();
    let old = s.focus;
    let Some(wd) = win(h) else { return old };
    let xs = x();
    if wd.mapped {
        let _ = xs.conn.set_input_focus(InputFocus::PARENT, wd.id, x11rb::CURRENT_TIME);
    } else {
        wd.want_focus = true;
    }
    xs.flush();
    old
}

pub unsafe extern "system" fn GetFocus() -> HWND {
    st().focus
}

pub unsafe extern "system" fn SetForegroundWindow(h: HWND) -> BOOL {
    let Some(wd) = win(h) else { return 0 };
    let xs = x();
    // source 2 (a pager) so focus stealing prevention lets it through
    xs.wm_message(wd.id, xs.atoms._NET_ACTIVE_WINDOW, [2, x11rb::CURRENT_TIME, 0, 0, 0]);
    xs.flush();
    1
}

pub unsafe extern "system" fn GetForegroundWindow() -> HWND {
    let xs = x();
    xs.prop32(xs.root, xs.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW).first().copied().unwrap_or(0) as HWND
}

pub unsafe extern "system" fn SetWinEventHook(_min: u32, _max: u32, _m: HINSTANCE, f: WINEVENTPROC, _pid: u32, _tid: u32, _flags: u32) -> HWINEVENTHOOK {
    let s = st();
    s.fg_hook = Some(f);
    s.active = GetForegroundWindow();
    1
}

pub unsafe extern "system" fn AllowSetForegroundWindow(_pid: u32) -> BOOL {
    1
}

// -------------------------------------------------------------------- pointer
pub unsafe extern "system" fn GetCursorPos(p: *mut POINT) -> BOOL {
    let xs = x();
    let Ok(Ok(r)) = xs.conn.query_pointer(xs.root).map(|c| c.reply()) else { return 0 };
    *p = POINT { x: r.root_x as i32, y: r.root_y as i32 };
    1
}

/// Our window under a screen point, if any (top-most first).
pub fn window_at(p: POINT) -> HWND {
    let xs = x();
    let Ok(Ok(r)) = xs.conn.query_pointer(xs.root).map(|c| c.reply()) else { return 0 };
    let _ = p;
    // The child of the root is a frame or our window; look inside frames.
    let mut w = r.child;
    for _ in 0..4 {
        if w == 0 || is_ours(w as HWND) {
            break;
        }
        let Ok(Ok(q)) = xs.conn.query_pointer(w).map(|c| c.reply()) else { break };
        w = q.child;
    }
    if is_ours(w as HWND) { w as HWND } else { 0 }
}

fn cursor_name(id: usize) -> &'static str {
    match id {
        IDC_IBEAM => "xterm",
        IDC_HAND => "hand2",
        IDC_SIZEALL => "fleur",
        IDC_SIZENWSE => "bottom_right_corner",
        32644 => "sb_h_double_arrow",
        32645 => "sb_v_double_arrow",
        32648 => "crossed_circle",
        32514 | 32650 => "watch",
        _ => "left_ptr",
    }
}

pub unsafe extern "system" fn LoadCursorW(_i: HINSTANCE, n: usize) -> HCURSOR {
    let s = st();
    if let Some(&c) = s.cursors.get(&n) {
        return c as HCURSOR;
    }
    let c = x().cursor(cursor_name(n));
    s.cursors.insert(n, c);
    c as HCURSOR
}

pub unsafe extern "system" fn LoadIconW(_i: HINSTANCE, _n: usize) -> isize {
    0
}

fn set_cursor_for(h: HWND, rx: i32, ry: i32) {
    let l = pack(rx, ry);
    let ht = unsafe { call(h, WM_NCHITTEST, 0, l) };
    let r = unsafe { call(h, WM_SETCURSOR, h as WPARAM, (ht & 0xFFFF) | ((WM_MOUSEMOVE as isize) << 16)) };
    if r == 0 {
        unsafe { DefWindowProcW(h, WM_SETCURSOR, h as WPARAM, 0) };
    }
}

pub unsafe extern "system" fn SetCursor(c: HCURSOR) -> HCURSOR {
    let s = st();
    let old = s.cursor;
    let target = if s.capture != 0 { s.capture } else { s.hover };
    if let Some(wd) = win(target) {
        if c != old || true {
            let _ = x().conn.change_window_attributes(wd.id, &ChangeWindowAttributesAux::new().cursor(c as u32));
        }
    }
    s.cursor = c;
    old
}

// ------------------------------------------------------------------- monitors
fn monitors() -> &'static Vec<(RECT, bool)> {
    let s = st();
    if s.monitors.is_none() {
        s.monitors = Some(read_monitors());
    }
    s.monitors.as_ref().unwrap()
}

fn read_monitors() -> Vec<(RECT, bool)> {
    let xs = x();
    let mut out: Vec<(RECT, bool)> = Vec::new();
    if xs.has_randr {
        use x11rb::protocol::randr::ConnectionExt as _;
        if let Ok(Ok(r)) = xs.conn.randr_get_monitors(xs.root, true).map(|c| c.reply()) {
            for m in r.monitors {
                let rc = RECT { left: m.x as i32, top: m.y as i32, right: m.x as i32 + m.width as i32, bottom: m.y as i32 + m.height as i32 };
                out.push((rc, m.primary));
            }
        }
        let _ = xs.conn.randr_select_input(xs.root, x11rb::protocol::randr::NotifyMask::SCREEN_CHANGE | x11rb::protocol::randr::NotifyMask::CRTC_CHANGE);
    }
    if out.is_empty() {
        let s = &xs.conn.setup().roots[xs.screen];
        out.push((RECT { left: 0, top: 0, right: s.width_in_pixels as i32, bottom: s.height_in_pixels as i32 }, true));
    }
    if !out.iter().any(|m| m.1) {
        out[0].1 = true;
    }
    out
}

pub fn workarea() -> Option<RECT> {
    let xs = x();
    let a = &xs.atoms;
    let wa = xs.prop32(xs.root, a._NET_WORKAREA, AtomEnum::CARDINAL);
    let d = xs.prop32(xs.root, a._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL).first().copied().unwrap_or(0) as usize;
    let v = wa.get(d * 4..d * 4 + 4).or_else(|| wa.get(0..4))?;
    Some(RECT { left: v[0] as i32, top: v[1] as i32, right: (v[0] + v[2]) as i32, bottom: (v[1] + v[3]) as i32 })
}

fn work_of(m: &RECT) -> RECT {
    match workarea() {
        Some(w) => {
            let r = RECT { left: m.left.max(w.left), top: m.top.max(w.top), right: m.right.min(w.right), bottom: m.bottom.min(w.bottom) };
            if r.right - r.left > 100 && r.bottom - r.top > 100 { r } else { *m }
        }
        None => *m,
    }
}

pub unsafe extern "system" fn MonitorFromPoint(p: POINT, _f: u32) -> HMONITOR {
    let ms = monitors();
    if let Some(i) = ms.iter().position(|(r, _)| p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom) {
        return i as HMONITOR + 1;
    }
    let dist = |r: &RECT| {
        let dx = (r.left - p.x).max(p.x - r.right + 1).max(0) as i64;
        let dy = (r.top - p.y).max(p.y - r.bottom + 1).max(0) as i64;
        dx * dx + dy * dy
    };
    ms.iter().enumerate().min_by_key(|(_, m)| dist(&m.0)).map_or(1, |(i, _)| i as HMONITOR + 1)
}

pub unsafe extern "system" fn MonitorFromWindow(h: HWND, f: u32) -> HMONITOR {
    let mut r = RECT::default();
    GetWindowRect(h, &mut r);
    MonitorFromPoint(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }, f)
}

pub unsafe extern "system" fn GetMonitorInfoW(m: HMONITOR, i: *mut MONITORINFO) -> BOOL {
    let ms = monitors();
    let Some((r, primary)) = ms.get((m as usize).wrapping_sub(1)).or_else(|| ms.first()) else { return 0 };
    let i = &mut *i;
    i.rcMonitor = *r;
    i.rcWork = work_of(r);
    i.dwFlags = u32::from(*primary);
    1
}

pub unsafe extern "system" fn SystemParametersInfoW(a: u32, _b: u32, p: *mut c_void, _f: u32) -> BOOL {
    if a == SPI_GETWORKAREA {
        let ms = monitors();
        let r = ms.iter().find(|m| m.1).map_or(RECT::default(), |m| m.0);
        *(p as *mut RECT) = work_of(&r);
        return 1;
    }
    0
}

pub unsafe extern "system" fn GetSystemMetrics(i: i32) -> i32 {
    let s = &x().conn.setup().roots[x().screen];
    match i {
        0 => s.width_in_pixels as i32,
        1 => s.height_in_pixels as i32,
        SM_CXDRAG | SM_CYDRAG => (4 * x().dpi / 96) as i32,
        36 | 37 => 4,
        _ => 0,
    }
}

pub unsafe extern "system" fn GetDpiForWindow(_h: HWND) -> u32 {
    x().dpi
}

pub unsafe extern "system" fn GetDpiForSystem() -> u32 {
    x().dpi
}

pub unsafe extern "system" fn SetProcessDpiAwarenessContext(_c: isize) -> BOOL {
    1
}

pub unsafe extern "system" fn GetDoubleClickTime() -> u32 {
    400
}

pub unsafe extern "system" fn GetCaretBlinkTime() -> u32 {
    530
}

// ---------------------------------------------------------------- drag & drop
pub unsafe extern "system" fn RegisterDragDrop(h: HWND, _t: *mut c_void) -> HRESULT {
    let Some(wd) = win(h) else { return -1 };
    let xs = x();
    let _ = xs.conn.change_property32(PropMode::REPLACE, wd.id, xs.atoms.XdndAware, AtomEnum::ATOM, &[5]);
    wd.dnd = true;
    0
}

pub unsafe extern "system" fn RevokeDragDrop(h: HWND) -> HRESULT {
    let Some(wd) = win(h) else { return -1 };
    let xs = x();
    let _ = xs.conn.delete_property(wd.id, xs.atoms.XdndAware);
    wd.dnd = false;
    0
}

pub unsafe extern "system" fn OleInitialize(_r: *mut c_void) -> HRESULT {
    0
}

/// A window to own selections and receive replies, created on first use.
pub fn utility_window() -> u32 {
    static mut W: u32 = 0;
    unsafe {
        if W == 0 {
            let xs = x();
            let id = xs.conn.generate_id().unwrap_or(0);
            let aux = CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE);
            let _ = xs.conn.create_window(0, id, xs.root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &aux);
            W = id;
        }
        W
    }
}
