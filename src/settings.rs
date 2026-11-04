//! Contents of the settings popover. Every change shows up on the folder
//! right away. Sizes that need icons to be redrawn apply when the slider is
//! released, so dragging stays smooth.

use crate::app::App;
use crate::config::Mode;
use crate::edit::Edit;
use crate::ffi::*;
use crate::popup::*;
use crate::uikit::*;
use crate::util::*;
use crate::i18n::tr;

pub const PRESETS: [u32; 10] = [
    0xFFF5B841, 0xFF3D8BFD, 0xFF34C759, 0xFFFF453A, 0xFF8E6CFF,
    0xFFFF6FAE, 0xFFFF9F0A, 0xFF32C8E8, 0xFF5B6270, 0xFFE3E8F0,
];
const TINTS: [u32; 8] = [
    0xFFFFFFFF, 0xFF1E1F24, 0xFFF5B841, 0xFF3D8BFD, 0xFF34C759, 0xFFFF6FAE, 0xFF8E6CFF, 0xFF32C8E8,
];

pub fn build(app: &mut App, st: &mut SettingsState, ui: &mut Ui) {
    let Some(i) = app.by_id(st.folder) else { return };
    folder_tab(app, i, st, ui);
    // everything that is not about this folder lives in the settings window
    ui.space(10.0);
    let (x, w) = (ui.x, ui.w);
    if ui.button(800, tr("pop.all_settings"), Some(Ic::Palette), 0, x, w) {
        app.close_popup(true);
        app.open_prefs();
    }
    ui.y += ui.s(34.0);
}

fn picker(app: &mut App, i: usize, st: &mut SettingsState, ui: &mut Ui, which: Pick, current: u32) {
    let Some((p, mut h, mut s, mut v)) = st.picker else { return };
    if p != which {
        return;
    }
    let changed = ui.hsv_picker(250, &mut h, &mut s, &mut v);
    st.picker = Some((p, h, s, v));
    let c = hsv(h, s, v);
    if changed {
        if st.focus != 2 {
            st.hex = Edit::new(&to_w(&hex(c)), 7, false);
        }
        // the SVG tint redraws icons, so it waits for the release
        if which != Pick::Tint {
            app.apply_pick(i, which, c);
        }
    }
    if which == Pick::Tint && (ui.released_id == 250 || ui.released_id == 251) {
        app.apply_pick(i, which, c);
    }
    let font = ui.fonts.get(ui.face, (13.0 * ui.sc) as i32, FW_NORMAL);
    if st.focus != 2 && st.hex.text.is_empty() {
        st.hex = Edit::new(&to_w(&hex(current)), 7, false);
    }
    if ui.field(260, &mut st.hex, st.focus == 2, font) {
        st.focus = 2;
    }
}

fn toggle_picker(st: &mut SettingsState, which: Pick, current: u32) {
    if matches!(st.picker, Some((p, ..)) if p == which) {
        st.picker = None;
    } else {
        let (h, s, v) = to_hsv(current);
        st.picker = Some((which, h, s, v));
        st.hex = Edit::new(&to_w(&hex(current)), 7, false);
    }
    st.focus = 0;
}

fn folder_tab(app: &mut App, i: usize, st: &mut SettingsState, ui: &mut Ui) {
    let font = ui.fonts.get(ui.face, (13.0 * ui.sc) as i32, FW_NORMAL);
    let title = if app.wins[i].cfg.mode == Mode::Panel { tr("pop.this_panel") } else { tr("pop.this_folder") };
    let (tx, ty) = (ui.x, ui.y + ui.s(6.0));
    let tc = ui.th.text;
    ui.text(title, tx, ty, 15.0, FW_SEMIBOLD, tc, false);
    ui.y += ui.s(22.0);
    ui.section(tr("pop.name"));
    if ui.field(100, &mut st.name, st.focus == 1, font) {
        st.focus = 1;
    }

    ui.section(tr("pop.type"));
    let mut mode = (app.wins[i].cfg.mode == Mode::Panel) as usize;
    if ui.segmented(110, &[tr("pop.type_folder"), tr("pop.type_panel")], &mut mode) {
        app.set_mode(i, mode == 1);
    }

    ui.section(tr("pop.color"));
    let color = app.wins[i].cfg.color;
    if let Some(c) = ui.swatches(200, &PRESETS, color, true) {
        if c == 0 {
            toggle_picker(st, Pick::Folder, color);
        } else {
            st.picker = None;
            app.apply_pick(i, Pick::Folder, c);
        }
    }
    picker(app, i, st, ui, Pick::Folder, color);

    ui.section(tr("pop.icon"));
    let has_icon = !app.wins[i].cfg.icon.is_empty();
    let half = (ui.w - ui.s(8.0)) / 2.0;
    let x0 = ui.x;
    let def = ui.button(300, tr("pop.icon_default"), Some(Ic::Folder), if has_icon { 0 } else { 1 }, x0, half);
    let pick = ui.button(301, tr("pop.icon_choose"), Some(Ic::Image), 0, x0 + half + ui.s(8.0), half);
    ui.y += ui.s(34.0 + 10.0);
    if def && has_icon {
        app.wins[i].cfg.icon.clear();
        app.refresh(i);
    }
    if pick {
        app.pick_folder_icon(i);
    }
    if ext_is(&to_w(&app.wins[i].cfg.icon), "svg") {
        ui.label(tr("pop.svg_color"));
        let tint = app.wins[i].cfg.tint;
        if let Some(c) = ui.swatches(320, &TINTS, tint, true) {
            if c == 0 {
                toggle_picker(st, Pick::Tint, tint);
            } else {
                st.picker = None;
                app.apply_pick(i, Pick::Tint, c);
            }
        }
        picker(app, i, st, ui, Pick::Tint, tint);
    }

    ui.section(tr("pop.header"));
    let mut hs = app.wins[i].cfg.header.unwrap_or(app.g.header) as usize;
    if ui.segmented(400, &[tr("pop.header_tint"), tr("pop.header_band"), tr("pop.header_line"), tr("pop.header_none")], &mut hs) {
        app.wins[i].cfg.header = Some(hs as u8);
        app.render(i);
        app.save();
    }

    ui.section(tr("pop.size"));
    let hover = app.wins[i].cfg.mode == Mode::Hover;
    if hover {
        size_slider(app, i, st, ui, 500, tr("pop.folder_icon"), 40.0, 128.0, 4.0);
    }
    size_slider(app, i, st, ui, 510, tr("pop.shortcut_icons"), 28.0, 96.0, 2.0);
    let cols = app.wins[i].cfg.columns as f32;
    if let Some(v) = ui.slider(520, tr("pop.columns"), cols, 1.0, 10.0, 1.0, "") {
        app.wins[i].cfg.columns = v as i32;
        app.relayout(i);
    }
    let rows = app.wins[i].cfg.rows as f32;
    if let Some(v) = ui.slider(530, if hover { tr("pop.rows_scroll") } else { tr("pop.rows_visible") }, rows, 1.0, 8.0, 1.0, "") {
        app.wins[i].cfg.rows = v as i32;
        app.relayout(i);
    }

    ui.section(tr("pop.behavior"));
    if hover {
        let mut lab = app.wins[i].cfg.label;
        if ui.toggle(600, tr("pop.show_label"), &mut lab) {
            app.wins[i].cfg.label = lab;
            app.refresh(i);
            app.save();
        }
    }
    let mut locked = app.wins[i].cfg.locked;
    if ui.toggle(610, tr("pop.lock"), &mut locked) {
        app.wins[i].cfg.locked = locked;
        app.save();
    }

    ui.space(12.0);
    // Nothing is lost here, the shortcuts go back to the desktop, so one click is enough.
    let label = if app.wins[i].cfg.mode == Mode::Panel { tr("pop.delete_panel") } else { tr("pop.delete_folder") };
    let (x, w) = (ui.x, ui.w);
    if ui.button(700, label, Some(Ic::Trash), 2, x, w) {
        app.delete_folder(i);
        return;
    }
    ui.y += ui.s(34.0);
}

/// Slider for sizes that need icons redrawn: shows the value live and applies
/// it when the mouse is released.
fn size_slider(app: &mut App, i: usize, st: &mut SettingsState, ui: &mut Ui, id: u32, label: &str, min: f32, max: f32, step: f32) {
    let f = &app.wins[i].cfg;
    let real = if id == 500 { f.size } else if f.icon_size > 0 { f.icon_size } else { app.g.icon_size } as f32;
    let shown = match st.preview {
        Some((pid, v)) if pid == id => v,
        _ => real,
    };
    if let Some(v) = ui.slider(id, label, shown, min, max, step, " px") {
        st.preview = Some((id, v));
    }
    if ui.released_id == id {
        if let Some((pid, v)) = st.preview.take() {
            if pid == id {
                if id == 500 {
                    app.wins[i].cfg.size = v as i32;
                } else {
                    app.wins[i].cfg.icon_size = v as i32;
                }
                app.refresh(i);
                app.save();
            }
        }
    }
}
