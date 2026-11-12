//! `CrowFO.exe --snapshot <dir>` draws the main states of the interface into PNG
//! files without creating any window. It is used to check the visuals and to
//! make screenshots, and it never touches the screen, mouse or keyboard.

use crate::app::{App, Fonts};
use crate::config::{Config, Mode};
use crate::edit::Edit;
use crate::ffi::*;
use crate::folder::{EditTarget, FolderWin};
use crate::gfx::{over, Canvas};
use crate::popup::{Kind, Pick, Slot};
use crate::store;
use crate::uikit::to_hsv;
use crate::util::*;
use alloc::boxed::Box;
use alloc::vec::Vec;

fn save(cv: &Canvas, dir: &[u16], name: &str) {
    // composite on a solid dark gray, like a plain desktop
    let n = (cv.w * cv.h) as usize;
    let src = unsafe { core::slice::from_raw_parts(cv.px, n) };
    let px: Vec<u32> = src.iter().map(|&c| over(0xFF202125, c)).collect();
    let path = wcat(&[dir, w!("\\"), &to_w(name)]);
    write_file(&path, &crate::art::png_wh(&px, cv.w, cv.h));
}

/// An App built from crow.ini with no real windows, for drawing offscreen.
fn headless() -> Option<&'static mut App> {
    let ini = store::ini_path();
    let data = read_file(&ini)?;
    let cfg = Config::parse(core::str::from_utf8(&data).unwrap_or(""));
    crate::i18n::set(&cfg.g.language);
    let sc = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
    let a: &'static mut App = Box::leak(Box::new(App {
        g: cfg.g,
        wins: Vec::new(),
        fonts: Fonts::new(),
        msg: 0,
        ini,
        last_ini: data,
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
        pc: store::computer_name(),
        popup: None,
        prefs: None,
        pending_drop: None,
        trimmed: false,
    }));
    for f in cfg.folders {
        let mut w = Box::new(FolderWin::new(f));
        w.sc = sc;
        w.place(&a.g);
        a.wins.push(w);
    }
    Some(a)
}

pub fn run(out: &[u16]) -> bool {
    mkdir(out);
    let Some(a) = headless() else { return false };
    let Some(i) = a.wins.iter().position(|w| w.cfg.mode == Mode::Hover).or(if a.wins.is_empty() { None } else { Some(0) }) else {
        return false;
    };

    // 1. collapsed folder
    a.wins[i].draw(&a.g, &mut a.fonts);
    if let Some(cv) = a.wins[i].frame() {
        save(cv, out, "1-collapsed.png");
    }

    // Frame cost of the opening animation, plus a few frames to look at.
    {
        let mut log = alloc::string::String::from("t, draw_us\n");
        let f = &mut a.wins[i];
        f.target = 1.0;
        f.place(&a.g);
        for k in 0..=20 {
            let f = &mut a.wins[i];
            f.t = k as f32 / 20.0;
            f.lid = f.t;
            let t0 = now_ms();
            a.wins[i].draw(&a.g, &mut a.fonts);
            let us = ((now_ms() - t0) * 1000.0) as i64;
            let t1 = now_ms();
            a.wins[i].draw(&a.g, &mut a.fonts);
            let us2 = ((now_ms() - t1) * 1000.0) as i64;
            fmt_int(k as i64 * 5, &mut log);
            log.push_str("%, ");
            fmt_int(us, &mut log);
            log.push_str(", cached ");
            fmt_int(us2, &mut log);
            log.push('\n');
            if k == 3 || k == 6 || k == 10 {
                if let Some(cv) = a.wins[i].frame() {
                    let mut name = alloc::string::String::from("anim-");
                    fmt_int(k as i64 * 5, &mut name);
                    name.push_str(".png");
                    save(cv, out, &name);
                }
            }
        }
        write_file(&wcat(&[out, w!("\\profile.txt")]), log.as_bytes());
    }

    // 2. open, with the settings button showing and a shortcut under the mouse
    {
        let f = &mut a.wins[i];
        f.t = 1.0;
        f.target = 1.0;
        f.lid = 1.0;
        f.gear_on = true;
        f.gear_a = 1.0;
        f.mouse_in = true;
        f.hover = 1;
        f.place(&a.g);
    }
    a.wins[i].draw(&a.g, &mut a.fonts);
    if let Some(cv) = a.wins[i].frame() {
        save(cv, out, "2-open.png");
    }

    // 3. renaming the folder in place
    {
        let f = &mut a.wins[i];
        f.hover = -1;
        let name = to_w(&f.cfg.name);
        let mut ed = Edit::new(&name, 80, false);
        ed.anchor = 0;
        ed.caret = name.len();
        f.edit = Some((EditTarget::Title, ed));
    }
    a.wins[i].draw(&a.g, &mut a.fonts);
    if let Some(cv) = a.wins[i].frame() {
        save(cv, out, "3-rename.png");
    }
    a.wins[i].edit = None;

    // 4 and 6. folder popover, plain and with the color picker open
    let shots: [(&str, bool); 2] = [("4-settings.png", false), ("6-picker.png", true)];
    for (name, picker) in shots {
        let mut p = a.make_settings(i);
        if let Kind::Settings(s) = &mut p.kind {
            if picker {
                let (h, sa, v) = to_hsv(a.wins[i].cfg.color);
                s.picker = Some((Pick::Folder, h, sa, v));
            }
        }
        p.t = 1.0;
        a.popup = Some(p);
        a.popup_pass(false);
        a.popup_pass(true);
        if let Some(p) = a.popup.take() {
            if let Some(cv) = p.cv.as_ref() {
                save(cv, out, name);
            }
        }
    }

    // 7 and 8. context menus
    let at = POINT { x: a.wins[i].panel.left + 40, y: a.wins[i].panel.top + 60 };
    for (name, item) in [("7-menu-item.png", Some(0usize)), ("8-menu-folder.png", None)] {
        if item.is_some() && a.wins[i].items.is_empty() {
            continue;
        }
        let mut p = a.make_menu(i, item, at);
        p.t = 1.0;
        a.popup = Some(p);
        a.popup_pass(true);
        if let Some(p) = a.popup.take() {
            if let Some(cv) = p.cv.as_ref() {
                save(cv, out, name);
            }
        }
    }

    // 9. the settings window, every page
    for page in 0..5usize {
        let mut p = a.make_prefs();
        if let Kind::Prefs(s) = &mut p.kind {
            s.page = page;
        }
        p.t = 1.0;
        a.prefs = Some(p);
        a.pass(Slot::Prefs, false);
        a.pass(Slot::Prefs, true);
        if let Some(p) = a.prefs.take() {
            if let Some(cv) = p.cv.as_ref() {
                let mut name = alloc::string::String::from("9-prefs-");
                fmt_int(page as i64, &mut name);
                name.push_str(".png");
                save(cv, out, &name);
            }
        }
    }

    // 10. a panel, open with the mouse in it, then rolled up
    if let Some(k) = a.wins.iter().position(|w| w.cfg.mode == Mode::Panel) {
        {
            let f = &mut a.wins[k];
            f.mouse_in = true;
            f.gear_on = true;
            f.gear_a = 1.0;
            f.place(&a.g);
        }
        a.wins[k].draw(&a.g, &mut a.fonts);
        if let Some(cv) = a.wins[k].frame() {
            save(cv, out, "10-panel.png");
        }
        // the same panel with the pointer near the resize corner
        a.wins[k].grip_near = true;
        a.wins[k].draw(&a.g, &mut a.fonts);
        if let Some(cv) = a.wins[k].frame() {
            save(cv, out, "10-panel-grip.png");
        }
        a.wins[k].grip_near = false;
        {
            let f = &mut a.wins[k];
            f.cfg.collapsed = true;
            f.roll = 0.0;
            f.place(&a.g);
        }
        a.wins[k].draw(&a.g, &mut a.fonts);
        if let Some(cv) = a.wins[k].frame() {
            save(cv, out, "11-panel-rolled.png");
        }
    }
    true
}

fn union(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

/// Screen position of the middle of each shortcut, found by hit testing.
fn item_centers(a: &App, i: usize) -> Vec<(f32, f32)> {
    let f = &a.wins[i];
    let mut boxes: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut y = f.panel.top;
    while y < f.panel.bottom {
        let mut x = f.panel.left;
        while x < f.panel.right {
            if let crate::folder::Hit::Item(j) = f.hit(&a.g, POINT { x: x - f.win.left, y: y - f.win.top }) {
                while boxes.len() <= j {
                    boxes.push((i32::MAX, i32::MAX, i32::MIN, i32::MIN));
                }
                let b = &mut boxes[j];
                *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
            x += 3;
        }
        y += 3;
    }
    boxes.iter().map(|b| ((b.0 + b.2) as f32 / 2.0, (b.1 + b.3) as f32 / 2.0)).collect()
}

/// `CrowFO.exe --demo <dir>` plays a short scripted session offscreen: the
/// pointer opens a folder, moves over its shortcuts, leaves so it closes, then
/// collapses and expands a panel. Folders react through the same hit testing,
/// timings and animation steps as the real app. Every 20 ms frame of the scene
/// is written as raw BGRA (f0000.bgra...), with the pointer in cursor.txt.
pub fn demo(out: &[u16]) -> bool {
    use crate::folder::Hit;
    mkdir(out);
    let Some(a) = headless() else { return false };
    let Some(fi) = a.wins.iter().position(|w| w.cfg.mode == Mode::Hover) else { return false };
    let Some(pi) = a.wins.iter().position(|w| w.cfg.mode == Mode::Panel) else { return false };

    // The scene covers every window in every state it goes through.
    let mut u = a.wins[pi].win;
    a.wins[fi].target = 1.0;
    a.wins[fi].t = 1.0;
    a.wins[fi].lid = 1.0;
    a.wins[fi].place(&a.g);
    u = union(u, a.wins[fi].win);
    let items = item_centers(a, fi);
    let open_panel = a.wins[fi].panel;
    a.wins[fi].target = 0.0;
    a.wins[fi].t = 0.0;
    a.wins[fi].lid = 0.0;
    a.wins[fi].place(&a.g);
    u = union(u, a.wins[fi].win);
    let pad = 8;
    let u = RECT { left: u.left - pad, top: u.top - pad, right: u.right + pad, bottom: u.bottom + pad };
    let (sw, sh) = (u.right - u.left, u.bottom - u.top);

    let ib = a.wins[fi].icon_box;
    let icon = ((ib.left + ib.right) as f32 / 2.0, (ib.top + ib.bottom) as f32 / 2.0 - 6.0);
    let chev = {
        let w = &a.wins[pi];
        let (x, y, cw, ch) = w.chevron_rect(&w.layout(&a.g));
        (w.win.left as f32 + x + cw / 2.0, w.win.top as f32 + y + ch / 2.0)
    };
    let start = (open_panel.right as f32 + 40.0, open_panel.bottom as f32 - 30.0);
    let outside = (open_panel.right as f32 + 50.0, (open_panel.top + open_panel.bottom) as f32 / 2.0);
    let it = |k: usize| items.get(k).copied().unwrap_or(icon);

    // Pointer keyframes (ms, x, y). Moves between them are eased.
    let keys: [(f32, (f32, f32)); 13] = [
        (0.0, start), (500.0, start), (1200.0, icon), (1700.0, icon),
        (2000.0, it(0)), (2350.0, it(0)), (2600.0, it(1)), (2950.0, it(1)),
        (3250.0, it(5)), (3600.0, it(5)), (4100.0, outside), (4900.0, chev), (8200.0, chev),
    ];
    let clicks = [5100.0f32, 6600.0];
    let end = 9200.0f32;
    let pos = |t: f32| -> (f32, f32) {
        if t >= keys[keys.len() - 1].0 {
            // glide back to where it started, so the GIF loops cleanly
            let k = clamp((t - 8200.0) / (end - 8200.0), 0.0, 1.0);
            let e = k * k * (3.0 - 2.0 * k);
            return (chev.0 + (start.0 - chev.0) * e, chev.1 + (start.1 - chev.1) * e);
        }
        let mut n = 0;
        while n + 1 < keys.len() && keys[n + 1].0 <= t {
            n += 1;
        }
        let (t0, p0) = keys[n];
        let (t1, p1) = keys[(n + 1).min(keys.len() - 1)];
        let k = if t1 > t0 { clamp((t - t0) / (t1 - t0), 0.0, 1.0) } else { 1.0 };
        let e = k * k * (3.0 - 2.0 * k);
        (p0.0 + (p1.0 - p0.0) * e, p0.1 + (p1.1 - p0.1) * e)
    };

    let dt = 20.0f32;
    let frames = (end / dt) as i32;
    let mut cursor = alloc::string::String::new();
    let mut rest_ms = 0.0f32;
    let mut away_ms = 0.0f32;
    let mut last = pos(0.0);
    let mut scene: Vec<u32> = alloc::vec![0; (sw * sh) as usize];
    for n in 0..frames {
        let t = n as f32 * dt;
        let c = pos(t);
        let moved = abs(c.0 - last.0) + abs(c.1 - last.1);
        last = c;
        rest_ms = if moved < 0.5 { rest_ms + dt } else { 0.0 };
        let mut pressed = 0;
        for &ct in &clicks {
            if t >= ct && t < ct + dt {
                // same as clicking the collapse button
                let f = &mut a.wins[pi];
                let collapsed = !f.cfg.collapsed;
                f.cfg.collapsed = false;
                f.place(&a.g);
                f.cfg.collapsed = collapsed;
            }
            if t >= ct - 40.0 && t < ct + 100.0 {
                pressed = 1;
            }
        }
        for k in 0..a.wins.len() {
            let f = &mut a.wins[k];
            let cp = POINT { x: c.0 as i32, y: c.1 as i32 };
            let pt = POINT { x: cp.x - f.win.left, y: cp.y - f.win.top };
            let h = if crate::app::in_rect(&f.win, cp) { f.hit(&a.g, pt) } else { Hit::None };
            f.mouse_in = h != Hit::None;
            f.hover = if let Hit::Item(j) = h { j as i32 } else { -1 };
            f.gear_on = matches!(h, Hit::Header | Hit::Title | Hit::Gear | Hit::Chevron);
            f.gear_hot = h == Hit::Gear;
            f.chev_hot = h == Hit::Chevron;
            f.grip_hot = h == Hit::Grip;
            f.grip_near = f.mouse_in && f.near_grip(&a.g, pt);
            if f.cfg.mode == Mode::Hover {
                // hover intent: open once the pointer rests on the icon, close
                // a moment after it leaves
                if f.target == 0.0 && h == Hit::Icon && rest_ms >= 40.0 {
                    f.target = 1.0;
                    f.place(&a.g);
                }
                away_ms = if f.target > 0.0 && h == Hit::None { away_ms + dt } else { 0.0 };
                if f.target > 0.0 && away_ms > a.g.close_delay as f32 {
                    f.target = 0.0;
                    f.hover = -1;
                }
            }
            let (t0, r0) = (f.t, f.roll);
            f.step(&a.g, dt);
            if f.cfg.mode == Mode::Hover && t0 > 0.0 && f.t == 0.0 && f.target == 0.0 {
                f.place(&a.g);
            }
            if f.cfg.mode == Mode::Panel && f.cfg.collapsed && r0 > 0.0 && f.roll <= 0.0 {
                f.place(&a.g);
            }
        }
        for p in scene.iter_mut() {
            *p = 0xFF202125;
        }
        for k in 0..a.wins.len() {
            a.wins[k].draw(&a.g, &mut a.fonts);
            let f = &a.wins[k];
            if let Some(cv) = f.frame() {
                let src = unsafe { core::slice::from_raw_parts(cv.px, (cv.w * cv.h) as usize) };
                let (ox, oy) = (f.win.left - u.left, f.win.top - u.top);
                for y in 0..cv.h {
                    let sy = oy + y;
                    if sy < 0 || sy >= sh {
                        continue;
                    }
                    for x in 0..cv.w {
                        let sx = ox + x;
                        if sx < 0 || sx >= sw {
                            continue;
                        }
                        let d = &mut scene[(sy * sw + sx) as usize];
                        *d = over(*d, src[(y * cv.w + x) as usize]);
                    }
                }
            }
        }
        let bytes = unsafe { core::slice::from_raw_parts(scene.as_ptr() as *const u8, scene.len() * 4) };
        let mut name = alloc::string::String::from("f");
        let mut num = alloc::string::String::new();
        fmt_int(n as i64 + 10000, &mut num);
        name.push_str(&num[1..]);
        name.push_str(".bgra");
        write_file(&wcat(&[out, w!("\\"), &to_w(&name)]), bytes);
        fmt_int((c.0 - u.left as f32) as i64, &mut cursor);
        cursor.push(' ');
        fmt_int((c.1 - u.top as f32) as i64, &mut cursor);
        cursor.push(' ');
        fmt_int(pressed, &mut cursor);
        cursor.push('\n');
    }
    let mut head = alloc::string::String::new();
    fmt_int(sw as i64, &mut head);
    head.push(' ');
    fmt_int(sh as i64, &mut head);
    head.push(' ');
    fmt_int(frames as i64, &mut head);
    head.push(' ');
    fmt_int(dt as i64, &mut head);
    head.push('\n');
    head.push_str(&cursor);
    write_file(&wcat(&[out, w!("\\cursor.txt")]), head.as_bytes());
    true
}
