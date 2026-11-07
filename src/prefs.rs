//! The Crow settings window: language, look, animation, defaults for new
//! folders and an about page. Changes apply live, like in the folder popover.

use crate::app::App;
use crate::edit::Edit;
use crate::ffi::*;
use crate::i18n::{self, tr};
use crate::popup::*;
use crate::settings::PRESETS;
use crate::uikit::*;
use crate::util::*;
use alloc::string::String;

const BACKGROUNDS: [u32; 8] = [
    0xFF1D1E23, 0xFF131417, 0xFF26282F, 0xFF1A2130, 0xFF261E2B, 0xFF1C2822, 0xFF2A2420, 0xFF000000,
];

/// Draws the whole window. Returns true when the user asked to close it.
pub fn build(app: &mut App, st: &mut PrefsState, ui: &mut Ui, m: f32, pw: f32, ph: f32, scroll: f32, content_h: &mut f32) -> bool {
    let sc = ui.sc;
    let bar = PREFS_BAR * sc;
    let side = PREFS_SIDE * sc;

    // title bar
    let mut close = false;
    if let Some(cv) = ui.cv.as_deref_mut() {
        let gs = (22.0 * sc) as i32;
        let px = crate::art::logo(gs);
        cv.blit(&px, gs, gs, m + 18.0 * sc, m + (bar - gs as f32) / 2.0, gs as f32, gs as f32, 256);
        cv.rrect(m, m + bar - 1.0 * sc, pw, 1.0 * sc, 0.0, ui.th.border, ui.th.border, 255, 1.0);
        cv.rrect(m + side, m + bar, 1.0 * sc, ph - bar, 0.0, ui.th.border, ui.th.border, 255, 1.0);
    }
    let tc = ui.th.text;
    ui.text_mid(tr("prefs.title"), m + 50.0 * sc, m, bar, 14.0, FW_SEMIBOLD, tc, false);
    let cs = 32.0 * sc;
    if ui.icon_button(9900, Ic::Close, m + pw - cs - 10.0 * sc, m + (bar - cs) / 2.0, cs) {
        close = true;
    }

    // sidebar
    let pages: [(Ic, &str); 5] = [
        (Ic::Gear, tr("prefs.general")),
        (Ic::Palette, tr("prefs.appearance")),
        (Ic::Clock, tr("prefs.animation")),
        (Ic::Plus, tr("prefs.new_folders")),
        (Ic::Info, tr("prefs.about")),
    ];
    ui.x = m + 10.0 * sc;
    ui.w = side - 20.0 * sc;
    ui.y = m + bar + 10.0 * sc;
    for (k, (ic, label)) in pages.iter().enumerate() {
        if ui.nav(9800 + k as u32, *ic, label, st.page == k) && st.page != k {
            st.page = k;
            st.picker = None;
            st.focus = 0;
        }
    }

    // content, scrolling under the title bar
    let top = m + bar;
    let bottom = m + ph - 8.0 * sc;
    ui.view = (top, bottom);
    if let Some(cv) = ui.cv.as_deref_mut() {
        cv.set_clip((m + side + 1.0) as i32, top as i32, (m + pw) as i32, bottom as i32);
    }
    ui.x = m + side + 24.0 * sc;
    ui.w = pw - side - 48.0 * sc;
    ui.y = top + 18.0 * sc - scroll;
    let start = ui.y;
    let title = pages[st.page].1;
    let (x, y) = (ui.x, ui.y);
    ui.text(title, x, y, 19.0, FW_SEMIBOLD, tc, false);
    ui.y += 30.0 * sc;
    match st.page {
        0 => general(app, ui),
        1 => appearance(app, st, ui),
        2 => animation(app, ui),
        3 => defaults(app, st, ui),
        _ => about(app, ui),
    }
    if ui.released_id != 0 || ui.inp.released {
        app.save();
    }
    *content_h = ui.y - start + 16.0 * sc;
    draw_scrollbar(ui, *content_h, scroll, top, bottom, m + pw - 7.0 * sc);
    if let Some(cv) = ui.cv.as_deref_mut() {
        cv.reset_clip();
    }
    close
}

fn general(app: &mut App, ui: &mut Ui) {
    ui.section(tr("prefs.language"));
    let cur = app.g.language.clone();
    let auto = cur.eq_ignore_ascii_case("auto") || cur.is_empty();
    let mut pick: Option<String> = None;
    if ui.choice(1200, tr("prefs.language_auto"), auto) && !auto {
        pick = Some("auto".into());
    }
    for (k, (code, name)) in i18n::available().iter().enumerate() {
        let sel = !auto && cur.eq_ignore_ascii_case(code);
        if ui.choice(1210 + k as u32, name, sel) && !sel {
            pick = Some(code.clone());
        }
    }
    if let Some(code) = pick {
        app.set_language(&code);
    }

    ui.section(tr("prefs.system"));
    let mut auto_start = app.g.autostart;
    if ui.toggle(1300, tr("prefs.startup"), &mut auto_start) {
        app.g.autostart = auto_start;
        crate::store::set_autostart(auto_start);
    }
    let mut dm = app.g.desktop_menu;
    if ui.toggle(1310, tr("prefs.desktop_menu"), &mut dm) {
        app.g.desktop_menu = dm;
        crate::store::set_desktop_menu(dm);
    }
    let mut top = app.g.on_top;
    if ui.toggle(1320, tr("prefs.on_top"), &mut top) {
        app.g.on_top = top;
        app.relayout_all();
    }
    let mut blur = app.g.blur;
    if ui.toggle(1330, tr("prefs.blur"), &mut blur) {
        app.g.blur = blur;
        app.render_all();
    }
}

fn picker(app: &mut App, st: &mut PrefsState, ui: &mut Ui, which: Pick, current: u32) {
    let Some((p, mut h, mut s, mut v)) = st.picker else { return };
    if p != which {
        return;
    }
    if ui.hsv_picker(1450, &mut h, &mut s, &mut v) {
        st.picker = Some((p, h, s, v));
        let c = hsv(h, s, v);
        if st.focus != 2 {
            st.hex = Edit::new(&to_w(&hex(c)), 7, false);
        }
        apply(app, which, c);
    }
    let font = ui.fonts.get(ui.face, (13.0 * ui.sc) as i32, FW_NORMAL);
    if st.focus != 2 && st.hex.text.is_empty() {
        st.hex = Edit::new(&to_w(&hex(current)), 7, false);
    }
    if ui.field(1460, &mut st.hex, st.focus == 2, font) {
        st.focus = 2;
    }
}

fn toggle_picker(st: &mut PrefsState, which: Pick, current: u32) {
    if matches!(st.picker, Some((p, ..)) if p == which) {
        st.picker = None;
    } else {
        let (h, s, v) = to_hsv(current);
        st.picker = Some((which, h, s, v));
        st.hex = Edit::new(&to_w(&hex(current)), 7, false);
    }
    st.focus = 0;
}

fn apply(app: &mut App, which: Pick, c: u32) {
    match which {
        Pick::Background => {
            app.g.bg = (app.g.bg & 0xFF00_0000) | (c & 0x00FF_FFFF);
            app.render_all();
        }
        Pick::DefaultColor => app.g.def_color = c | 0xFF00_0000,
        _ => {}
    }
}

/// The hex field of the color picker was edited.
pub fn hex_changed(app: &mut App) {
    let Some(Popup { kind: Kind::Prefs(st), .. }) = app.prefs.as_deref_mut() else { return };
    let text = from_w(&st.hex.text);
    let Some((which, ..)) = st.picker else { return };
    if text.len() == 7 {
        if let Some(c) = crate::svg::parse_color(text.as_bytes()) {
            let (h, s, v) = to_hsv(c);
            st.picker = Some((which, h, s, v));
            apply(app, which, c);
        }
    }
}

fn appearance(app: &mut App, st: &mut PrefsState, ui: &mut Ui) {
    ui.section(tr("prefs.background"));
    let bg = app.g.bg;
    if let Some(c) = ui.swatches(1400, &BACKGROUNDS, bg, true) {
        if c == 0 {
            toggle_picker(st, Pick::Background, bg | 0xFF00_0000);
        } else {
            st.picker = None;
            apply(app, Pick::Background, c);
        }
    }
    picker(app, st, ui, Pick::Background, bg | 0xFF00_0000);
    let op = round((app.g.bg >> 24) as f32 * 100.0 / 255.0);
    if let Some(v) = ui.slider(1500, tr("prefs.opacity"), op, 40.0, 100.0, 1.0, "%") {
        let a = (v * 255.0 / 100.0 + 0.5) as u32;
        app.g.bg = (app.g.bg & 0x00FF_FFFF) | (a.min(255) << 24);
        app.render_all();
    }
    if let Some(v) = ui.slider(1510, tr("prefs.radius"), app.g.radius as f32, 0.0, 28.0, 1.0, " px") {
        app.g.radius = v as i32;
        app.render_all();
    }

    ui.section(tr("pop.header"));
    let mut hs = app.g.header as usize;
    if ui.segmented(1520, &[tr("pop.header_tint"), tr("pop.header_band"), tr("pop.header_line"), tr("pop.header_none")], &mut hs) {
        app.g.header = hs as u8;
        app.render_all();
    }
    if let Some(v) = ui.slider(1530, tr("prefs.header_strength"), app.g.header_strength as f32, 0.0, 100.0, 1.0, "%") {
        app.g.header_strength = v as i32;
        app.render_all();
    }

    ui.section(tr("pop.size"));
    let shown = match st.preview {
        Some((1540, v)) => v,
        _ => app.g.font_size as f32,
    };
    if let Some(v) = ui.slider(1540, tr("prefs.text_size"), shown, 9.0, 16.0, 1.0, " px") {
        st.preview = Some((1540, v));
    }
    let shown_icon = match st.preview {
        Some((1550, v)) => v,
        _ => app.g.icon_size as f32,
    };
    if let Some(v) = ui.slider(1550, tr("pop.shortcut_icons"), shown_icon, 28.0, 96.0, 2.0, " px") {
        st.preview = Some((1550, v));
    }
    if ui.released_id == 1540 || ui.released_id == 1550 {
        if let Some((id, v)) = st.preview.take() {
            if id == 1540 {
                app.g.font_size = v as i32;
                app.g.title_size = v as i32 + 1;
            } else {
                app.g.icon_size = v as i32;
            }
            app.relayout_all();
        }
    }
    let mut sh = app.g.shadow;
    if ui.toggle(1560, tr("prefs.shadow"), &mut sh) {
        app.g.shadow = sh;
        app.render_all();
    }
}

fn animation(app: &mut App, ui: &mut Ui) {
    if let Some(v) = ui.slider(1600, tr("prefs.duration"), app.g.anim_ms as f32, 80.0, 500.0, 10.0, " ms") {
        app.g.anim_ms = v as u32;
    }
    if let Some(v) = ui.slider(1610, tr("prefs.open_after"), app.g.hover_delay as f32, 0.0, 600.0, 10.0, " ms") {
        app.g.hover_delay = v as u32;
    }
    if let Some(v) = ui.slider(1620, tr("prefs.close_after"), app.g.close_delay as f32, 0.0, 1200.0, 20.0, " ms") {
        app.g.close_delay = v as u32;
    }
}

fn defaults(app: &mut App, st: &mut PrefsState, ui: &mut Ui) {
    let mc = ui.th.muted;
    ui.paragraph(tr("prefs.new_hint"), 12.5, mc);
    ui.section(tr("prefs.default_color"));
    let mut auto = app.g.def_color == 0;
    if ui.toggle(1700, tr("prefs.color_auto"), &mut auto) {
        app.g.def_color = if auto { 0 } else { PRESETS[0] };
        st.picker = None;
    }
    if !auto {
        let c = app.g.def_color;
        if let Some(n) = ui.swatches(1710, &PRESETS, c, true) {
            if n == 0 {
                toggle_picker(st, Pick::DefaultColor, c);
            } else {
                st.picker = None;
                app.g.def_color = n;
            }
        }
        picker(app, st, ui, Pick::DefaultColor, c);
    }
    ui.section(tr("pop.size"));
    if let Some(v) = ui.slider(1720, tr("prefs.default_folder_icon"), app.g.def_size as f32, 40.0, 128.0, 4.0, " px") {
        app.g.def_size = v as i32;
    }
    if let Some(v) = ui.slider(1730, tr("prefs.default_columns"), app.g.def_columns as f32, 1.0, 10.0, 1.0, "") {
        app.g.def_columns = v as i32;
    }
}

fn about(app: &mut App, ui: &mut Ui) {
    let sc = ui.sc;
    if let Some(cv) = ui.cv.as_deref_mut() {
        let gs = (64.0 * sc) as i32;
        let px = crate::art::logo(gs);
        cv.blit(&px, gs, gs, ui.x, ui.y, gs as f32, gs as f32, 256);
    }
    let (x, y) = (ui.x + 80.0 * sc, ui.y + 8.0 * sc);
    let (tc, mc) = (ui.th.text, ui.th.muted);
    ui.text("Crow FO", x, y, 22.0, FW_SEMIBOLD, tc, false);
    let mut v = String::from(tr("prefs.version"));
    v.push(' ');
    v.push_str(env!("CROW_VERSION"));
    ui.text(&v, x, y + 32.0 * sc, 13.0, FW_NORMAL, mc, false);
    let by = tr("prefs.created_by").replace("{}", env!("CARGO_PKG_AUTHORS"));
    ui.text(&by, x, y + 52.0 * sc, 13.0, FW_NORMAL, mc, false);
    ui.y += 92.0 * sc;
    ui.paragraph(tr("prefs.translate"), 13.0, mc);
    ui.space(6.0);
    let half = (ui.w - ui.s(8.0)) / 2.0;
    let x0 = ui.x;
    let x1 = x0 + half + ui.s(8.0);
    let site = ui.button(1890, tr("prefs.project_page"), Some(Ic::GitHub), 0, x0, half);
    let ini = ui.button(1900, tr("prefs.edit_ini"), Some(Ic::File), 0, x1, half);
    ui.y += ui.s(42.0);
    let quit = ui.button(1910, tr("prefs.quit"), Some(Ic::Power), 2, x0, half);
    ui.y += ui.s(34.0);
    if site {
        let url = wz(&to_w(env!("CARGO_PKG_REPOSITORY")));
        unsafe { ShellExecuteW(0, core::ptr::null(), url.as_ptr(), core::ptr::null(), core::ptr::null(), SW_SHOWNORMAL) };
    }
    if ini {
        app.open_ini();
    }
    if quit {
        unsafe { PostQuitMessage(0) };
    }
}
