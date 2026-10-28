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

pub fn run(out: &[u16]) -> bool {
    mkdir(out);
    let ini = store::ini_path();
    let Some(data) = read_file(&ini) else { return false };
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
