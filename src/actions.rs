//! What the menu and the settings popover can do, and renaming in place.

use crate::app::*;
use crate::config::{Folder, Mode};
use crate::edit::{Edit, Key};
use crate::ffi::*;
use crate::folder::EditTarget;
use crate::popup::{Act, Kind, Pick, Popup};
use crate::settings::PRESETS;
use crate::store;
use crate::util::*;

pub const T_CARET: usize = 4;

impl App {
    /// Rebuilds a folder's cached content and redraws it.
    pub fn refresh(&mut self, i: usize) {
        let f = &mut self.wins[i];
        f.invalidate_content();
        f.place(&self.g);
        self.render(i);
    }
    /// Recomputes the layout (columns, rows) without touching cached icons.
    pub fn relayout(&mut self, i: usize) {
        let f = &mut self.wins[i];
        let l = f.layout(&self.g);
        let max = (l.rows_total - l.rows_vis).max(0) as f32;
        f.scroll = f.scroll.min(max);
        f.scroll_target = f.scroll_target.min(max);
        f.title_changed();
        f.place(&self.g);
        self.render(i);
    }
    pub fn render_all(&mut self) {
        for i in 0..self.wins.len() {
            self.render(i);
        }
    }

    pub fn open_ini(&mut self) {
        self.save();
        let p = wz(&self.ini);
        unsafe {
            ShellExecuteW(0, w!("open").as_ptr(), w!("notepad.exe").as_ptr(), p.as_ptr(), core::ptr::null(), SW_SHOWNORMAL);
        }
    }

    pub fn apply_pick(&mut self, i: usize, which: Pick, c: u32) {
        match which {
            Pick::Folder => {
                self.wins[i].cfg.color = c | 0xFF00_0000;
                self.wins[i].glyph_changed();
                self.render(i);
            }
            Pick::Tint => {
                self.wins[i].cfg.tint = c | 0xFF00_0000;
                self.refresh(i);
            }
            Pick::Background => {
                self.g.bg = (self.g.bg & 0xFF00_0000) | (c & 0x00FF_FFFF);
                self.render_all();
            }
            Pick::DefaultColor => self.g.def_color = c | 0xFF00_0000,
        }
    }

    pub fn set_mode(&mut self, i: usize, panel: bool) {
        let hwnd = self.wins[i].hwnd;
        let f = &mut self.wins[i];
        let m = if panel { Mode::Panel } else { Mode::Hover };
        if f.cfg.mode == m {
            return;
        }
        if panel {
            // the panel stays exactly where it was shown
            f.cfg.x = f.panel.left;
            f.cfg.y = f.panel.top;
            f.t = 1.0;
            f.target = 1.0;
        } else {
            f.t = 1.0;
            f.target = 1.0;
        }
        f.cfg.mode = m;
        f.scroll = 0.0;
        f.scroll_target = 0.0;
        self.refresh(i);
        if panel {
            self.sink(hwnd);
        }
        self.save();
    }

    pub fn delete_folder(&mut self, i: usize) {
        if self.wins.len() <= 1 {
            return;
        }
        self.close_popup(false);
        let n = self.wins[i].items.len();
        for j in 0..n {
            let p = self.wins[i].item_path(j);
            store::export_to_desktop(&p);
        }
        let dir = self.wins[i].dir.clone();
        self.remove_folder(i);
        unsafe {
            DeleteFileW(wz(&store::IconCache::path(&dir)).as_ptr());
            RemoveDirectoryW(wz(&dir).as_ptr());
        }
        self.save();
    }

    /// Creates a folder where the mouse is and starts editing its name.
    pub fn new_folder_at_cursor(&mut self, panel: bool) {
        let c = cursor();
        let id = self.next_id();
        let mut f = Folder::new(id, if panel { crate::i18n::tr("panel.new") } else { crate::i18n::tr("folder.new") }, self.snap(c.x - 46), self.snap(c.y - 20));
        // the defaults chosen in the settings window
        f.color = if self.g.def_color != 0 { self.g.def_color } else { PRESETS[(id as usize) % PRESETS.len()] };
        f.size = self.g.def_size;
        f.columns = self.g.def_columns;
        if panel {
            f.mode = Mode::Panel;
            f.columns = self.g.def_columns.min(6);
            f.rows = 2;
        }
        let k = self.add_folder(f);
        self.save();
        if !panel {
            self.expand(k);
        }
        self.start_edit(k, EditTarget::Title, None);
    }

    /// Runs the system file picker while keeping the popover open.
    fn pick_image(&mut self, owner: HWND) -> Option<WString> {
        if let Some(p) = self.popup.as_mut() {
            p.modal = true;
        }
        let r = crate::ui::pick_image(owner);
        if let Some(p) = self.popup.as_mut() {
            p.modal = false;
            unsafe {
                SetForegroundWindow(p.hwnd);
            }
        }
        r
    }

    pub fn pick_folder_icon(&mut self, i: usize) {
        let hwnd = self.wins[i].hwnd;
        let owner = self.popup.as_ref().map_or(hwnd, |p| p.hwnd);
        if let Some(path) = self.pick_image(owner) {
            if let Some(i) = self.by_hwnd(hwnd) {
                self.wins[i].cfg.icon = from_w(&path);
                self.refresh(i);
                self.save();
            }
        }
    }

    pub fn run_menu_act(&mut self, a: Act, fid: u32, item: Option<usize>) {
        let Some(i) = self.by_id(fid) else { return };
        let item = item.filter(|&j| j < self.wins[i].items.len());
        match (a, item) {
            (Act::Open, Some(j)) => {
                store::launch(&self.wins[i].item_path(j));
                self.collapse(i);
            }
            (Act::Reveal, Some(j)) => store::reveal(&self.wins[i].item_path(j)),
            (Act::RenameItem, Some(j)) => self.start_edit(i, EditTarget::Item(j), None),
            (Act::ItemIcon, Some(j)) => {
                let name = from_w(&self.wins[i].items[j]);
                let hwnd = self.wins[i].hwnd;
                if let Some(img) = self.pick_image(hwnd) {
                    let f = &mut self.wins[i].cfg;
                    f.item_icons.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
                    f.item_icons.push((name, from_w(&img)));
                    self.refresh(i);
                    self.save();
                }
            }
            (Act::ItemIconReset, Some(j)) => {
                let name = from_w(&self.wins[i].items[j]);
                self.wins[i].cfg.item_icons.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
                self.refresh(i);
                self.save();
            }
            (Act::Export, Some(j)) => {
                if store::export_to_desktop(&self.wins[i].item_path(j)) {
                    let f = &mut self.wins[i];
                    f.items.remove(j);
                    f.sync_order();
                    self.refresh(i);
                    self.save();
                }
            }
            (Act::Delete, Some(j)) => self.delete_item(i, j),
            (Act::Settings, _) => self.open_settings(i),
            (Act::Collapse, _) => self.toggle_collapse(i),
            (Act::AppSettings, _) => self.open_prefs(),
            (Act::RenameFolder, _) => self.start_edit(i, EditTarget::Title, None),
            (Act::NewFolder, _) => self.new_folder_at_cursor(false),
            (Act::NewPanel, _) => self.new_folder_at_cursor(true),
            _ => {}
        }
    }

    /// Sends a shortcut to the Recycle Bin, so it can still be restored.
    pub fn delete_item(&mut self, i: usize, j: usize) {
        if j >= self.wins[i].items.len() {
            return;
        }
        if store::recycle(&self.wins[i].item_path(j)) {
            let f = &mut self.wins[i];
            f.items.remove(j);
            f.sync_order();
            self.refresh(i);
            self.save();
        }
    }

    /// Rolls a panel up to its header, or back down.
    pub fn toggle_collapse(&mut self, i: usize) {
        let f = &mut self.wins[i];
        if f.cfg.mode != Mode::Panel {
            return;
        }
        f.cfg.collapsed = !f.cfg.collapsed;
        // keep the window at full size while the animation runs
        let collapsed = f.cfg.collapsed;
        f.cfg.collapsed = false;
        f.place(&self.g);
        f.cfg.collapsed = collapsed;
        self.render(i);
        self.save();
    }

    /// Switches the interface language and redraws what shows text.
    pub fn set_language(&mut self, code: &str) {
        self.g.language = code.into();
        crate::i18n::set(code);
        if self.g.desktop_menu {
            store::set_desktop_menu(true);
        }
        if let Some(p) = self.prefs.as_ref() {
            let t = wz(&to_w(crate::i18n::tr("prefs.title")));
            unsafe { SetWindowTextW(p.hwnd, t.as_ptr()) };
        }
        self.relayout_all();
        self.save();
    }

    // ------------------------------------------------- renaming in place
    /// Starts editing a folder title or a shortcut name right where it is shown.
    /// `click_x` places the caret under the mouse instead of selecting all.
    pub fn start_edit(&mut self, i: usize, target: EditTarget, click_x: Option<i32>) {
        if self.wins[i].edit.is_some() {
            self.commit_edit(i, true);
        }
        if self.wins[i].cfg.mode == Mode::Hover && self.wins[i].target == 0.0 {
            self.expand(i);
        }
        let f = &mut self.wins[i];
        let text = match target {
            EditTarget::Title => to_w(&f.cfg.name),
            EditTarget::Item(j) => match f.items.get(j) {
                Some(n) => stem(n).to_vec(),
                None => return,
            },
        };
        let mut ed = Edit::new(&text, if matches!(target, EditTarget::Item(_)) { 120 } else { 80 }, matches!(target, EditTarget::Item(_)));
        if let Some(x) = click_x {
            let font = self.fonts.get(&self.g.font, f.edit_font_px(&self.g, target), crate::folder::edit_weight(target));
            let fx = f.edit_rect(&self.g, target).0 + f.edit_pad();
            let at = ed.hit(font, fx, x as f32);
            ed.caret = at;
            ed.anchor = at;
        }
        f.edit = Some((target, ed));
        let hwnd = f.hwnd;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_NOACTIVATE) as isize);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            SetTimer(hwnd, T_CARET, 265, 0);
            KillTimer(hwnd, T_CLOSE);
        }
        self.render(i);
    }

    /// Ends the edit, saving it when `keep` is set.
    pub fn commit_edit(&mut self, i: usize, keep: bool) {
        let Some((target, ed)) = self.wins[i].edit.take() else { return };
        let hwnd = self.wins[i].hwnd;
        unsafe {
            KillTimer(hwnd, T_CARET);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_NOACTIVATE) as isize);
        }
        let name = from_w(&ed.text);
        let name = name.trim();
        if keep && !name.is_empty() {
            match target {
                EditTarget::Title => {
                    if self.wins[i].cfg.name != name {
                        self.wins[i].cfg.name = name.into();
                        self.wins[i].title_changed();
                        self.update_settings_name(i);
                    }
                }
                EditTarget::Item(j) if j < self.wins[i].items.len() => {
                    let old = self.wins[i].items[j].clone();
                    if trim_nul(stem(&old)) != &to_w(name)[..] {
                        let dir = self.wins[i].dir.clone();
                        if let Some(newname) = store::rename(&dir, &old, &to_w(name)) {
                            let f = &mut self.wins[i];
                            let (o, n) = (from_w(&old), from_w(&newname));
                            for e in f.cfg.item_icons.iter_mut() {
                                if e.0.eq_ignore_ascii_case(&o) {
                                    e.0 = n.clone();
                                }
                            }
                            f.items[j] = newname;
                            f.sync_order();
                            f.invalidate_content();
                        }
                    }
                }
                _ => {}
            }
            self.save();
        }
        self.wins[i].place(&self.g);
        self.render(i);
        let f = &self.wins[i];
        if f.cfg.mode == Mode::Panel {
            self.sink(hwnd);
        } else if !in_rect(&f.panel, cursor()) && !in_rect(&f.icon_box, cursor()) && !self.popup_open_for(f.cfg.id) {
            unsafe { SetTimer(hwnd, T_CLOSE, self.g.close_delay.max(1), 0) };
        }
    }

    /// Keyboard input for a folder that is being renamed.
    pub fn edit_key(&mut self, i: usize, vk: usize) {
        let hwnd = self.wins[i].hwnd;
        let Some((_, ed)) = self.wins[i].edit.as_mut() else { return };
        match ed.on_key(vk, hwnd) {
            Key::Commit => self.commit_edit(i, true),
            Key::Cancel => self.commit_edit(i, false),
            Key::None => {}
            _ => self.render(i),
        }
    }
    pub fn edit_char(&mut self, i: usize, c: u16) {
        let Some((_, ed)) = self.wins[i].edit.as_mut() else { return };
        if !matches!(ed.on_char(c), Key::None) {
            self.render(i);
        }
    }

    /// Keeps the popover's name field in sync after renaming in place.
    fn update_settings_name(&mut self, i: usize) {
        let id = self.wins[i].cfg.id;
        let name = to_w(&self.wins[i].cfg.name);
        if let Some(Popup { kind: Kind::Settings(s), .. }) = self.popup.as_deref_mut() {
            if s.folder == id && s.focus != 1 {
                s.name = Edit::new(&name, 80, false);
            }
        }
    }

    /// The popover's name or color field changed.
    pub fn settings_text_changed(&mut self) {
        let Some(Popup { kind: Kind::Settings(s), .. }) = self.popup.as_deref_mut() else { return };
        let fid = s.folder;
        let name = from_w(&s.name.text);
        let hex_text = from_w(&s.hex.text);
        let pick = s.picker.map(|p| p.0);
        let Some(i) = self.by_id(fid) else { return };
        let name = name.trim();
        if !name.is_empty() && self.wins[i].cfg.name != name {
            self.wins[i].cfg.name = name.into();
            self.wins[i].title_changed();
            self.render(i);
        }
        if let (Some(which), Some(c)) = (pick, crate::svg::parse_color(hex_text.as_bytes())) {
            if hex_text.len() == 7 {
                self.apply_pick(i, which, c);
                if let Some(Popup { kind: Kind::Settings(s), .. }) = self.popup.as_deref_mut() {
                    let (h, sa, v) = crate::uikit::to_hsv(c);
                    s.picker = Some((which, h, sa, v));
                }
            }
        }
    }
}
