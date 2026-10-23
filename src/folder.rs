//! Each organizer folder is one layered window with per-pixel alpha.
//! Only state lives here permanently. Every pixel buffer (canvas, labels and
//! decoded icons) is created on demand and dropped when the folder goes idle.

use crate::config::{Folder, Global, Mode};
use crate::ffi::*;
use crate::gfx::*;
use crate::store::{self, IconCache};
use crate::text::{self, Mask};
use crate::util::*;
use crate::edit::Edit;
use crate::{glyph, image};
use alloc::vec::Vec;

/// What is being renamed in place.
#[derive(Clone, Copy, PartialEq)]
pub enum EditTarget {
    Title,
    Item(usize),
}
pub fn edit_weight(t: EditTarget) -> i32 {
    if t == EditTarget::Title { FW_SEMIBOLD } else { FW_NORMAL }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Hit {
    None,
    Icon,
    Header,
    Title,
    Gear,
    Chevron,
    Item(usize),
    Grip,
    Panel,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Drag {
    None,
    Pending(Hit, POINT),
    Move(POINT, POINT), // cursor start, cfg pos start
    Resize(POINT, i32, i32),
    Item(usize),
    /// Selecting text with the mouse inside a name being edited.
    EditSelect,
}

/// Cell paddings in logical pixels: above the icon, between icon and label, and below the label.
pub const CELL_TOP: i32 = 7;
pub const CELL_GAP: i32 = 4;
pub const CELL_BOT: i32 = 7;
/// Space between the header and the first row (logical px).
pub const HEADER_GAP: i32 = 10;

pub struct Lay {
    pub pad: i32,
    pub header: i32,
    /// Where the grid starts, which is the header plus some breathing room
    pub body: i32,
    pub cell_w: i32,
    pub cell_h: i32,
    pub icon: i32,
    pub cols: i32,
    pub rows_total: i32,
    pub rows_vis: i32,
    pub pw: i32,
    pub ph: i32,
    pub gs: i32,
    pub box_w: i32,
    pub box_h: i32,
    pub margin: i32,
    pub hg: i32,
}

pub struct FolderWin {
    pub cfg: Folder,
    pub hwnd: HWND,
    pub dir: WString,
    pub items: Vec<WString>,
    pub sc: f32,
    pub t: f32,
    pub target: f32,
    pub lid: f32,
    pub scroll: f32,
    pub scroll_target: f32,
    pub mouse_in: bool,
    pub drop_hover: bool,
    pub hover: i32,
    pub press: i32,
    pub drag: Drag,
    pub insert_at: i32,
    /// Label lines needed by the longest name (1 or 2). Zero means not measured yet.
    pub lines: i32,
    pub lh: i32,
    pub icon_box: RECT,
    pub panel: RECT,
    pub win: RECT,
    pub backdrop: HWND,
    /// Name being edited in place, if any.
    pub edit: Option<(EditTarget, Edit)>,
    /// Settings button: current opacity and whether it should be shown.
    pub gear_a: f32,
    pub gear_on: bool,
    pub gear_hot: bool,
    /// Hover intent tracking: when the mouse entered and how fast it moves.
    pub enter_t: f64,
    pub last_move: (i32, i32, f64),
    pub speed: f32,
    /// Panels only: 1 when fully open, 0 when rolled up to the header.
    pub roll: f32,
    pub grip_hot: bool,
    /// The pointer is close to the resize corner, so the grip is shown.
    pub grip_near: bool,
    pub chev_hot: bool,
    hdr_px: i32,
    full_px: i32,
    cache: IconCache,
    keys: Vec<u64>,
    folder_key: u64,
    labels: Vec<Mask>,
    title: Option<Mask>,
    count: Option<Mask>,
    name: Option<(Mask, Mask)>,
    glyph: Option<(i32, i32, Vec<u32>)>,
    canvas: Option<Canvas>,
}

fn ir(r: &RECT) -> (f32, f32, f32, f32) {
    (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32)
}

impl FolderWin {
    pub fn new(cfg: Folder) -> FolderWin {
        let dir = store::folder_dir(cfg.id);
        let mut f = FolderWin {
            cfg,
            hwnd: 0,
            dir,
            items: Vec::new(),
            sc: 1.0,
            t: 0.0,
            target: 0.0,
            lid: 0.0,
            scroll: 0.0,
            scroll_target: 0.0,
            mouse_in: false,
            drop_hover: false,
            hover: -1,
            press: -1,
            drag: Drag::None,
            insert_at: -1,
            lines: 0,
            lh: 0,
            icon_box: RECT::default(),
            panel: RECT::default(),
            win: RECT::default(),
            backdrop: 0,
            edit: None,
            gear_a: 0.0,
            gear_on: false,
            gear_hot: false,
            enter_t: 0.0,
            last_move: (0, 0, 0.0),
            speed: 0.0,
            roll: 1.0,
            grip_hot: false,
            grip_near: false,
            chev_hot: false,
            hdr_px: 0,
            full_px: 0,
            cache: IconCache::empty(),
            keys: Vec::new(),
            folder_key: 0,
            labels: Vec::new(),
            title: None,
            count: None,
            name: None,
            glyph: None,
            canvas: None,
        };
        if f.cfg.collapsed {
            f.roll = 0.0;
        }
        if f.cfg.mode == Mode::Panel {
            f.t = 1.0;
            f.target = 1.0;
        }
        f.rescan();
        f
    }

    /// Re-reads the folder directory and reconciles it with the saved order.
    pub fn rescan(&mut self) {
        let files = list_files(&self.dir);
        let mut items: Vec<WString> = Vec::new();
        for o in &self.cfg.order {
            let w = to_w(o);
            if files.iter().any(|f| weq_ic(f, &w)) && !items.iter().any(|i| weq_ic(i, &w)) {
                items.push(w);
            }
        }
        for f in files {
            if !items.iter().any(|i| weq_ic(i, &f)) && f.first() != Some(&(b'.' as u16)) && !weq_ic(&f, w!("desktop.ini")) {
                items.push(f);
            }
        }
        self.items = items;
        self.sync_order();
        self.invalidate_content();
    }
    /// True when the directory no longer matches the item list, for example after a sync.
    pub fn dir_changed(&self) -> bool {
        let files: Vec<WString> = list_files(&self.dir).into_iter()
            .filter(|f| f.first() != Some(&(b'.' as u16)) && !weq_ic(f, w!("desktop.ini")))
            .collect();
        files.len() != self.items.len() || files.iter().any(|f| !self.items.iter().any(|i| weq_ic(i, f)))
    }
    pub fn sync_order(&mut self) {
        self.cfg.order = self.items.iter().map(|i| from_w(i)).collect();
    }
    /// Drops cached text/icons so they are rebuilt on next render.
    pub fn invalidate_content(&mut self) {
        self.keys.clear();
        self.labels.clear();
        self.title = None;
        self.count = None;
        self.name = None;
        self.lines = 0;
        self.glyph = None;
        self.cache.close();
    }
    /// Releases every pixel buffer. Called when idle.
    pub fn release(&mut self) {
        // The label measurement only depends on the items, so keep it.
        let (lines, lh) = (self.lines, self.lh);
        self.invalidate_content();
        self.canvas = None;
        self.lines = lines;
        self.lh = lh;
    }
    pub fn item_path(&self, i: usize) -> WString {
        wcat(&[&self.dir, w!("\\"), &self.items[i]])
    }
    pub fn icon_copy(&self, g: &Global, i: usize) -> Option<(Vec<u32>, i32)> {
        let l = self.layout(g);
        let k = *self.keys.get(i)?;
        self.cache.get(k, l.icon).map(|p| (p.to_vec(), l.icon))
    }
    pub fn animating(&self) -> bool {
        let lid_t = self.lid_target();
        self.t != self.target || abs(self.lid - lid_t) > 0.001 || abs(self.scroll - self.scroll_target) > 0.001
            || abs(self.gear_a - self.gear_target()) > 0.001
            || abs(self.roll - self.roll_target()) > 0.001
    }
    fn roll_target(&self) -> f32 {
        if self.cfg.mode == Mode::Panel && self.cfg.collapsed { 0.0 } else { 1.0 }
    }
    /// Panels that are rolled up (or rolling) only show and accept their header.
    pub fn rolled(&self) -> bool {
        self.cfg.mode == Mode::Panel && (self.cfg.collapsed || self.roll < 0.999)
    }
    /// Collapse button rect for panels, just left of the settings button.
    pub fn chevron_rect(&self, l: &Lay) -> (f32, f32, f32, f32) {
        let (x, y, w, h) = self.gear_rect(l);
        (x - w - 2.0 * self.sc, y, w, h)
    }
    fn gear_target(&self) -> f32 {
        if (self.gear_on || self.gear_hot) && self.e() > 0.9 { 1.0 } else { 0.0 }
    }
    /// Forgets the rendered title so it is drawn again (name or size changed).
    pub fn title_changed(&mut self) {
        self.title = None;
        self.name = None;
        self.labels.clear();
        self.lines = 0;
    }
    pub fn glyph_changed(&mut self) {
        self.glyph = None;
    }
    /// Loads icons and text ahead of time so opening starts instantly.
    pub fn prewarm(&mut self, g: &Global, fonts: &mut crate::Fonts) {
        let l = self.layout(g);
        self.ensure_icons(&l);
        self.ensure_text(g, &l, fonts);
    }

    // ------------------------------------------------ geometry helpers
    /// Settings button rect, window relative.
    pub fn gear_rect(&self, l: &Lay) -> (f32, f32, f32, f32) {
        let s = 28.0 * self.sc;
        let x = (self.panel.left - self.win.left) as f32 + l.pw as f32 - l.pad as f32 - s + 4.0 * self.sc;
        let y = (self.panel.top - self.win.top) as f32 + (l.header as f32 - s) / 2.0;
        (x, y, s, s)
    }
    /// Where the title text starts, window relative.
    fn title_x(&self, l: &Lay) -> f32 {
        (self.panel.left - self.win.left) as f32 + l.pad as f32 + l.hg as f32 + 10.0 * self.sc
    }
    pub fn edit_pad(&self) -> f32 {
        7.0 * self.sc
    }
    pub fn edit_font_px(&self, g: &Global, t: EditTarget) -> i32 {
        let px = if t == EditTarget::Title { g.title_size } else { g.font_size };
        (px as f32 * self.sc) as i32
    }
    /// Field rect for an in-place rename, window relative.
    pub fn edit_rect(&self, g: &Global, t: EditTarget) -> (f32, f32, f32, f32) {
        let l = self.layout(g);
        let sc = self.sc;
        match t {
            EditTarget::Title => {
                let x = self.title_x(&l) - self.edit_pad();
                let gear = self.gear_rect(&l).0;
                let h = 28.0 * sc;
                let y = (self.panel.top - self.win.top) as f32 + (l.header as f32 - h) / 2.0;
                (x, y, (gear - 6.0 * sc - x).max(40.0 * sc), h)
            }
            EditTarget::Item(i) => {
                let (cx, cy) = self.cell(&l, i);
                let lh = self.lh.max(1) as f32;
                let y = cy as f32 + (CELL_TOP as f32 * sc) + l.icon as f32 + (CELL_GAP as f32 * sc) - 4.0 * sc;
                (cx as f32 + 2.0 * sc, y, l.cell_w as f32 - 4.0 * sc, lh + 8.0 * sc)
            }
        }
    }
    fn lid_target(&self) -> f32 {
        if self.target > 0.5 && self.cfg.mode == Mode::Hover {
            1.0
        } else if self.drop_hover {
            0.75
        } else if self.mouse_in && self.cfg.mode == Mode::Hover {
            0.4
        } else {
            0.0
        }
    }
    pub fn expanded(&self) -> bool {
        self.cfg.mode == Mode::Panel || self.t > 0.0 || self.target > 0.0
    }

    pub fn layout(&self, g: &Global) -> Lay {
        let sc = self.sc;
        let s = |v: i32| (v as f32 * sc + 0.5) as i32;
        let icon = s(if self.cfg.icon_size > 0 { self.cfg.icon_size } else { g.icon_size });
        let lh = if self.lh > 0 { self.lh } else { (g.font_size as f32 * sc * 1.34 + 0.5) as i32 };
        let lines = if self.lines > 0 { self.lines } else { 2 };
        let pad = s(10);
        let header = s(g.title_size + 24);
        let cell_w = s(g.cell_w).max(icon + s(16));
        let cell_h = s(CELL_TOP) + icon + s(CELL_GAP) + lh * lines + s(CELL_BOT);
        let n = self.items.len() as i32;
        let (cols, rows_vis, rows_total);
        if self.cfg.mode == Mode::Panel {
            cols = self.cfg.columns.max(1);
            rows_total = ((n + cols - 1) / cols).max(1);
            rows_vis = self.cfg.rows.max(1);
        } else {
            cols = self.cfg.columns.min(n.max(self.cfg.columns.min(4))).max(1);
            rows_total = ((n + cols - 1) / cols).max(1);
            rows_vis = rows_total.min(self.cfg.rows.max(1));
        }
        let pw = cols * cell_w + pad * 2;
        let body = header + s(HEADER_GAP);
        let ph = body + rows_vis * cell_h + pad;
        let gs = s(self.cfg.size);
        let box_w = (gs + s(16)).max(s(92));
        let box_h = s(6) + gs + if self.cfg.label { s(4) + lh * 2 } else { s(6) };
        Lay { pad, header, body, cell_w, cell_h, icon, cols, rows_total, rows_vis, pw, ph, gs, box_w, box_h, margin: s(24), hg: (header as f32 * 0.66) as i32 }
    }

    /// Computes icon box / panel / window rects (screen coords).
    pub fn place(&mut self, g: &Global) {
        if self.lines == 0 {
            let l0 = self.layout(g);
            let (lines, lh) = text::measure(&self.items, &g.font, (g.font_size as f32 * self.sc) as i32,
                l0.cell_w - (6.0 * self.sc) as i32);
            self.lines = lines;
            self.lh = lh;
        }
        let l = self.layout(g);
        self.hdr_px = l.header;
        self.full_px = l.ph;
        unsafe {
            let (bw, bh) = if self.cfg.mode == Mode::Panel { (l.pw, l.ph) } else { (l.box_w, l.box_h) };
            let mut mi = MONITORINFO { cbSize: core::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(MonitorFromPoint(POINT { x: self.cfg.x + bw / 2, y: self.cfg.y + bh / 2 }, MONITOR_DEFAULTTONEAREST), &mut mi);
            let wa = mi.rcWork;
            // never leave a folder off-screen (other PC, other resolution, unplugged monitor)
            self.cfg.x = self.cfg.x.min(wa.right - bw).max(wa.left);
            self.cfg.y = self.cfg.y.min(wa.bottom - bh).max(wa.top);
            let (x, y) = (self.cfg.x, self.cfg.y);
            let m = (8.0 * self.sc) as i32;
            if self.cfg.mode == Mode::Panel {
                let h = if self.cfg.collapsed && self.roll <= 0.001 { l.header } else { l.ph };
                self.panel = RECT { left: x, top: y, right: x + l.pw, bottom: y + h };
                self.icon_box = self.panel;
            } else {
                self.icon_box = RECT { left: x, top: y, right: x + l.box_w, bottom: y + l.box_h };
                let cx = x + l.box_w / 2;
                let mut px = cx - l.pw / 2;
                px = px.min(wa.right - m - l.pw).max(wa.left + m);
                let mut py = y;
                if py + l.ph > wa.bottom - m {
                    py = (y + l.box_h - l.ph).max(wa.top + m);
                }
                self.panel = RECT { left: px, top: py, right: px + l.pw, bottom: py + l.ph };
            }
            let u = if self.expanded() {
                RECT {
                    left: self.icon_box.left.min(self.panel.left),
                    top: self.icon_box.top.min(self.panel.top),
                    right: self.icon_box.right.max(self.panel.right),
                    bottom: self.icon_box.bottom.max(self.panel.bottom),
                }
            } else {
                self.icon_box
            };
            self.win = RECT { left: u.left - l.margin, top: u.top - l.margin, right: u.right + l.margin, bottom: u.bottom + l.margin };
        }
    }

    /// Eased open amount used for geometry.
    pub fn e(&self) -> f32 {
        if self.cfg.mode == Mode::Panel {
            return 1.0;
        }
        if self.target > 0.5 { ease_out_quart(self.t) } else { 1.0 - ease_out_cubic(1.0 - self.t) }
    }

    /// Current animated panel rect, window-relative.
    pub fn cur_rect(&self) -> (f32, f32, f32, f32) {
        if self.cfg.mode == Mode::Panel {
            let (bx, by, bw, _) = ir(&self.panel);
            let l = self.layout_cached();
            let h = lerp(l.0 as f32, l.1 as f32, ease_in_out_cubic(self.roll));
            return (bx - self.win.left as f32, by - self.win.top as f32, bw, h);
        }
        let e = self.e();
        let (ax, ay, aw, ah) = ir(&self.icon_box);
        let (bx, by, bw, bh) = ir(&self.panel);
        let (ox, oy) = (self.win.left as f32, self.win.top as f32);
        (lerp(ax, bx, e) - ox, lerp(ay, by, e) - oy, lerp(aw, bw, e), lerp(ah, bh, e))
    }

    /// (header height, full panel height) from the last placement.
    fn layout_cached(&self) -> (i32, i32) {
        (self.hdr_px, self.full_px)
    }

    /// Item cell rect, window-relative (before scroll clipping).
    fn cell(&self, l: &Lay, i: usize) -> (i32, i32) {
        let col = i as i32 % l.cols;
        let row = i as i32 / l.cols;
        let x = self.panel.left - self.win.left + l.pad + col * l.cell_w;
        let y = self.panel.top - self.win.top + l.body + ((row as f32 - self.scroll) * l.cell_h as f32) as i32;
        (x, y)
    }

    /// Whether the pointer (window relative) is near the bottom right corner of
    /// an open panel. The resize grip only shows up there.
    pub fn near_grip(&self, g: &Global, pt: POINT) -> bool {
        if self.cfg.mode != Mode::Panel || self.cfg.locked || self.rolled() {
            return false;
        }
        let l = self.layout(g);
        let (rx, ry) = (pt.x + self.win.left - self.panel.left, pt.y + self.win.top - self.panel.top);
        let near = (44.0 * self.sc) as i32;
        rx > l.pw - near && ry > l.ph - near && rx < l.pw && ry < l.ph
    }

    pub fn hit(&self, g: &Global, pt: POINT) -> Hit {
        // pt is window-relative
        let l = self.layout(g);
        let inside = |r: &RECT| {
            let (x, y) = (pt.x + self.win.left, pt.y + self.win.top);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        };
        if !self.expanded() || (self.cfg.mode == Mode::Hover && self.t < 0.98 && self.target < 0.5) {
            return if inside(&self.icon_box) { Hit::Icon } else { Hit::None };
        }
        if !inside(&self.panel) {
            return if inside(&self.icon_box) { Hit::Icon } else { Hit::None };
        }
        let (rx, ry) = (pt.x + self.win.left - self.panel.left, pt.y + self.win.top - self.panel.top);
        let grip = (18.0 * self.sc) as i32;
        if self.cfg.mode == Mode::Panel && !self.rolled() && rx > l.pw - grip && ry > l.ph - grip {
            return Hit::Grip;
        }
        if ry < l.header {
            let (gx, gy, gw, gh) = self.gear_rect(&l);
            let (px, py) = (pt.x as f32, pt.y as f32);
            if px >= gx && px < gx + gw && py >= gy && py < gy + gh {
                return Hit::Gear;
            }
            if self.cfg.mode == Mode::Panel {
                let (cx, cy, cw, ch) = self.chevron_rect(&l);
                if px >= cx && px < cx + cw && py >= cy && py < cy + ch {
                    return Hit::Chevron;
                }
            }
            let tx = self.title_x(&l);
            let tw = self.title.as_ref().map_or(80.0 * self.sc, |m| m.w as f32).max(40.0 * self.sc);
            if px >= tx - 4.0 * self.sc && px < tx + tw + 6.0 * self.sc {
                return Hit::Title;
            }
            return Hit::Header;
        }
        if self.rolled() {
            return Hit::Panel;
        }
        if let Some(i) = self.item_at(&l, pt, false) {
            return Hit::Item(i);
        }
        Hit::Panel
    }

    /// Item under a window-relative point. With `insert`, returns the
    /// insertion index for drag-reordering instead.
    pub fn item_at(&self, l: &Lay, pt: POINT, insert: bool) -> Option<usize> {
        let x = pt.x + self.win.left - self.panel.left - l.pad;
        let y = pt.y + self.win.top - self.panel.top - l.body;
        if y < 0 || y >= l.rows_vis * l.cell_h || x < 0 && !insert {
            return if insert { Some(if y < 0 { 0 } else { self.items.len() }) } else { None };
        }
        let fy = y as f32 / l.cell_h as f32 + self.scroll;
        let row = fy as i32;
        if insert {
            let col = ((x as f32 / l.cell_w as f32) + 0.5) as i32;
            let idx = (row * l.cols + col.clamp(0, l.cols)) as usize;
            return Some(idx.min(self.items.len()));
        }
        let col = x / l.cell_w;
        if col >= l.cols {
            return None;
        }
        let i = (row * l.cols + col) as usize;
        if i < self.items.len() { Some(i) } else { None }
    }

    /// Advances animation by dt ms. Returns true while still animating.
    pub fn step(&mut self, g: &Global, dt: f32) -> bool {
        let dur = g.anim_ms.max(1) as f32;
        if self.t < self.target {
            self.t = (self.t + dt / dur).min(self.target);
        } else if self.t > self.target {
            self.t = (self.t - dt / (dur * 0.8)).max(self.target);
        }
        let rt = self.roll_target();
        let rs = dt / (dur * 1.1);
        self.roll = if self.roll < rt { (self.roll + rs).min(rt) } else { (self.roll - rs).max(rt) };
        let gt = self.gear_target();
        let gs = dt / 140.0;
        self.gear_a = if self.gear_a < gt { (self.gear_a + gs).min(gt) } else { (self.gear_a - gs).max(gt) };
        let lt = self.lid_target();
        let ls = dt / (dur * 0.7);
        if self.lid < lt {
            self.lid = (self.lid + ls).min(lt);
        } else if self.lid > lt {
            self.lid = (self.lid - ls).max(lt);
        }
        let d = self.scroll_target - self.scroll;
        if abs(d) > 0.001 {
            let k = clamp(dt / 90.0, 0.0, 1.0);
            self.scroll += d * k;
            if abs(self.scroll_target - self.scroll) < 0.002 {
                self.scroll = self.scroll_target;
            }
        }
        self.animating()
    }

    fn icon_key(&self, i: usize, s: i32) -> u64 {
        let name = &self.items[i];
        let custom = self.cfg.item_icon(&from_w(name));
        let mut h = fnv_w(0, name);
        let src = match custom {
            Some(c) => {
                h = fnv(h, c.as_bytes());
                h = fnv(h, &self.cfg.tint.to_le_bytes());
                to_w(c)
            }
            None => self.item_path(i),
        };
        h = fnv(h, &mtime(&src).to_le_bytes());
        fnv(h, &s.to_le_bytes())
    }

    /// Makes sure all icons for the current layout are in the mmap cache.
    fn ensure_icons(&mut self, l: &Lay) {
        let fk = if self.cfg.icon.is_empty() {
            0
        } else {
            let h = fnv(fnv(0, b"folder"), self.cfg.icon.as_bytes());
            let h = fnv(h, &self.cfg.tint.to_le_bytes());
            fnv(fnv(h, &mtime(&to_w(&self.cfg.icon)).to_le_bytes()), &l.gs.to_le_bytes())
        };
        if self.keys.len() == self.items.len() && self.folder_key == fk && self.cache.is_open() {
            return;
        }
        self.keys = (0..self.items.len()).map(|i| self.icon_key(i, l.icon)).collect();
        self.folder_key = fk;
        if !self.cache.is_open() {
            self.cache = IconCache::open(&self.dir);
        }
        let missing = self.keys.iter().any(|&k| self.cache.get(k, l.icon).is_none())
            || (fk != 0 && self.cache.get(fk, l.gs).is_none());
        if !missing && self.cache.is_open() {
            return;
        }
        let mut entries: Vec<(u64, i32, Vec<u32>)> = Vec::with_capacity(self.items.len() + 1);
        for i in 0..self.items.len() {
            let k = self.keys[i];
            let px = match self.cache.get(k, l.icon) {
                Some(p) => p.to_vec(),
                None => {
                    let custom = self.cfg.item_icon(&from_w(&self.items[i])).map(to_w);
                    let r = match custom {
                        Some(c) => image::load_file(&c, l.icon, self.cfg.tint),
                        None => None,
                    };
                    r.or_else(|| image::shell_icon(&self.item_path(i), l.icon)).unwrap_or_else(|| glyph::fallback(l.icon))
                }
            };
            entries.push((k, l.icon, px));
        }
        if fk != 0 {
            let px = match self.cache.get(fk, l.gs) {
                Some(p) => Some(p.to_vec()),
                None => image::load_file(&to_w(&self.cfg.icon), l.gs, self.cfg.tint),
            };
            if let Some(px) = px {
                entries.push((fk, l.gs, px));
            }
        }
        self.cache.close();
        store::write_cache(&self.dir, &entries);
        self.cache = IconCache::open(&self.dir);
    }

    /// Folder glyph position and size (window relative) at open amount `e`.
    fn glyph_pos(&self, l: &Lay, e: f32) -> (f32, f32, f32) {
        let sc = self.sc;
        let (ox, oy) = (self.win.left as f32, self.win.top as f32);
        let to = (self.panel.left as f32 - ox + l.pad as f32, self.panel.top as f32 - oy + (l.header - l.hg) as f32 / 2.0, l.hg as f32);
        if self.cfg.mode == Mode::Panel {
            return to;
        }
        let bw = (self.icon_box.right - self.icon_box.left) as f32;
        let from = (self.icon_box.left as f32 - ox + (bw - l.gs as f32) / 2.0, self.icon_box.top as f32 - oy + 6.0 * sc, l.gs as f32);
        (lerp(from.0, to.0, e), lerp(from.1, to.1, e), lerp(from.2, to.2, e))
    }

    /// Draws the folder glyph (default vector folder or custom image).
    fn draw_glyph(&mut self, cv: &mut Canvas, x: f32, y: f32, size: f32, alpha: u32, l: &Lay) {
        let gs = l.gs;
        if self.folder_key != 0 {
            if let Some(px) = self.cache.get(self.folder_key, gs) {
                let bump = 1.0 + 0.07 * ease_out_cubic(self.lid);
                let s2 = size * bump;
                cv.blit(px, gs, gs, x - (s2 - size) / 2.0, y - (s2 - size) / 2.0, s2, s2, alpha);
                return;
            }
        }
        let panel = self.cfg.mode == Mode::Panel;
        let state = if panel { 1.0 - self.roll } else { self.lid };
        let q = (state * 48.0) as i32; // quantized so the cached glyph can be reused
        let rs = (size as i32).max(8);
        let regen = match &self.glyph {
            Some((gq, gsz, _)) => *gq != q || *gsz != rs,
            None => true,
        };
        if regen {
            let st = ease_out_cubic(q as f32 / 48.0);
            let px = if panel { glyph::panel(rs, self.cfg.color, st) } else { glyph::folder(rs, self.cfg.color, st) };
            self.glyph = Some((q, rs, px));
        }
        let (_, _, px) = self.glyph.as_ref().unwrap();
        cv.blit(px, rs, rs, x, y, size, size, alpha);
    }

    fn ensure_text(&mut self, g: &Global, l: &Lay, fonts: &mut crate::Fonts) {
        if self.lines == 0 {
            let (lines, lh) = text::measure(&self.items, &g.font, (g.font_size as f32 * self.sc) as i32,
                l.cell_w - (6.0 * self.sc) as i32);
            self.lines = lines;
            self.lh = lh;
        }
        if self.labels.len() != self.items.len() {
            let f = fonts.get(&g.font, (g.font_size as f32 * self.sc) as i32, FW_NORMAL);
            self.labels = self.items.iter().map(|it| text::render(stem(it), f, l.cell_w - (6.0 * self.sc) as i32, self.lines.max(1), true)).collect();
        }
        if self.count.is_none() && g.show_count && !self.items.is_empty() {
            let f = fonts.get(&g.font, ((g.font_size as f32 - 1.0) * self.sc) as i32, FW_SEMIBOLD);
            let mut s = alloc::string::String::new();
            fmt_int(self.items.len() as i64, &mut s);
            self.count = Some(text::render(&to_w(&s), f, (60.0 * self.sc) as i32, 1, false));
        }
        if self.title.is_none() {
            let f = fonts.get(&g.font, (g.title_size as f32 * self.sc) as i32, FW_SEMIBOLD);
            let badge = self.count.as_ref().map_or(0, |m| m.w + (24.0 * self.sc) as i32);
            let w = l.pw - l.pad * 2 - l.hg - (10.0 * self.sc) as i32 - badge;
            self.title = Some(text::render(&to_w(&self.cfg.name), f, w.max(10), 1, false));
        }
        if self.name.is_none() && self.cfg.mode == Mode::Hover && self.cfg.label {
            let f = fonts.get(&g.font, (g.font_size as f32 * self.sc) as i32, FW_SEMIBOLD);
            let m = text::render(&to_w(&self.cfg.name), f, l.box_w, 2, true);
            let s = text::shadow(&m);
            self.name = Some((m, s));
        }
    }

    pub fn render(&mut self, g: &Global, fonts: &mut crate::Fonts) {
        if self.hwnd == 0 {
            return;
        }
        self.draw(g, fonts);
        if let Some(cv) = self.canvas.take() {
            self.present(&cv);
            self.canvas = Some(cv);
        }
    }

    /// The last drawn frame (used by --snapshot).
    pub fn frame(&self) -> Option<&Canvas> {
        self.canvas.as_ref()
    }

    /// Draws the folder into its canvas without showing it.
    pub fn draw(&mut self, g: &Global, fonts: &mut crate::Fonts) {
        let l = self.layout(g);
        let (ww, wh) = (self.win.right - self.win.left, self.win.bottom - self.win.top);
        let reuse = matches!(&self.canvas, Some(c) if c.w == ww && c.h == wh);
        if !reuse {
            self.canvas = None;
            self.canvas = Canvas::dib(ww, wh);
        }
        let Some(mut cv) = self.canvas.take() else { return };
        cv.reset_clip();
        cv.clear(0);
        self.ensure_icons(&l);
        self.ensure_text(g, &l, fonts);
        let sc = self.sc;
        let e = self.e();
        let (ox, oy) = (self.win.left as f32, self.win.top as f32);
        let (bx, by, bw, _bh) = ir(&self.icon_box);
        let (bx, by) = (bx - ox, by - oy);

        // ---- collapsed look (fades out while opening)
        let collapsed_a = if self.cfg.mode == Mode::Panel { 0.0 } else { clamp(1.0 - e * 2.5, 0.0, 1.0) };
        if collapsed_a > 0.0 {
            let a = (collapsed_a * 256.0) as u32;
            if self.drop_hover {
                let gl = l.gs as f32 * 1.25;
                cv.rrect(bx + (bw - gl) / 2.0, by + (6.0 * sc) - (gl - l.gs as f32) / 2.0, gl, gl, gl / 2.0, 0x60FFFFFF, 0x60FFFFFF, 255, gl * 0.25);
            }
            if let Some((m, s)) = &self.name {
                let ty = (by + 10.0 * sc + l.gs as f32) as i32;
                let tx = (bx + (bw - m.w as f32) / 2.0) as i32;
                if g.label_shadow {
                    cv.mask(&s.a, s.w, s.h, tx - 2, ty - 1, scale(0xD0000000, a));
                }
                cv.mask(&m.a, m.w, m.h, tx, ty, scale(pm(g.text, 255), a));
            }
        }

        // ---- expanded panel
        if e > 0.001 {
            let (rx, ry, rw, rh) = self.cur_rect();
            let r = lerp(l.gs as f32 * 0.3, g.radius as f32 * sc, e);
            // The card becomes solid almost at once and grows from the icon.
            // Only the content fades, which also keeps every frame cheap.
            let pa = (clamp(e * 5.0, 0.0, 1.0) * 255.0) as u32;
            if g.shadow {
                let opaque = (if self.cfg.bg != 0 { self.cfg.bg } else { g.bg }) >> 24 == 255;
                if opaque {
                    cv.shadow(rx, ry + 8.0 * sc, rw, rh, r, 0x70000000, pa, 22.0 * sc);
                } else {
                    cv.rrect(rx, ry + 8.0 * sc, rw, rh, r, 0x70000000, 0x70000000, pa, 22.0 * sc);
                }
            }
            let bg = if self.cfg.bg != 0 { self.cfg.bg } else { g.bg };
            cv.rrect(rx, ry, rw, rh, r, bg, bg, pa, 1.0);
            cv.rrect_stroke(rx, ry, rw, rh, r, 1.0 * sc, g.border, (g.border & 0x00FF_FFFF) | ((g.border >> 25) << 24), pa);

            let (px0, py0) = (self.panel.left as f32 - ox, self.panel.top as f32 - oy);
            // ---- Header. It is set apart by color, surface and type instead of a divider line.
            let ta = clamp((e - 0.35) / 0.65, 0.0, 1.0);
            let hs = self.cfg.header.unwrap_or(g.header);
            let k = g.header_strength as f32 / 100.0;
            let acc = self.cfg.color & 0x00FF_FFFF;
            let hdr = l.header as f32;
            let full = [rx as i32, ry as i32, (rx + rw) as i32, (ry + rh) as i32];
            cv.set_clip(full[0], full[1], full[2], full[3]);
            match hs {
                0 => {
                    // Tint. The folder color fades down from the top edge.
                    let a = (k * 255.0).min(255.0) as u32;
                    cv.rrect(rx, ry, rw, hdr, r, acc | a << 24, acc, pa, 1.0);
                }
                1 => {
                    // Band. The header is a slightly lighter surface and the content sits on the darker base.
                    let a = (k * 0.45 * 255.0) as u32;
                    if rh > hdr + r {
                        cv.set_clip(full[0], full[1], full[2], ((py0 + hdr) as i32).min(full[3]));
                        cv.rrect(rx, ry, rw, hdr + r + 2.0, r, 0x00FF_FFFF | a << 24, 0x00FF_FFFF | (a * 2 / 3) << 24, pa, 1.0);
                        cv.set_clip(full[0], full[1], full[2], full[3]);
                    } else {
                        cv.rrect(rx, ry, rw, rh, r, 0x00FF_FFFF | a << 24, 0x00FF_FFFF | (a * 2 / 3) << 24, pa, 1.0);
                    }
                }
                2 => {
                    // Line. A classic thin divider.
                    let lc = 0x00FF_FFFF | ((g.border >> 24) / 2) << 24;
                    let la = ta * clamp((rh - hdr) / (4.0 * sc), 0.0, 1.0);
                    cv.rrect(px0 + l.pad as f32, py0 + hdr - 1.0 * sc, l.pw as f32 - 2.0 * l.pad as f32, 1.0 * sc, 0.0, lc, lc, (la * 255.0) as u32, 1.0);
                }
                _ => {} // none, spacing alone separates the header
            }
            let editing_title = matches!(self.edit, Some((EditTarget::Title, _)));
            if let Some(m) = self.title.as_ref().filter(|_| !editing_title) {
                // the title rides along to the right of the moving folder icon
                let (gx, gy, gsz) = self.glyph_pos(&l, e);
                let tx = (gx + gsz + 10.0 * sc) as i32;
                let ty = (gy + (gsz - m.h as f32) / 2.0) as i32;
                let a = clamp((e - 0.45) / 0.55, 0.0, 1.0);
                cv.mask(&m.a, m.w, m.h, tx, ty, pm(g.title, (ease_out_cubic(a) * 255.0) as u32));
            }
            if let Some(m) = &self.count {
                // Item count in a quiet pill, with lower contrast because it is secondary
                let (bw, bh) = (m.w as f32 + 14.0 * sc, m.h as f32 + 4.0 * sc);
                let bx = px0 + l.pw as f32 - l.pad as f32 - bw - 30.0 * sc;
                let by = py0 + (hdr - bh) / 2.0;
                let pc = if hs == 0 { acc | 0x3800_0000 } else { 0x1CFF_FFFF };
                cv.rrect(bx, by, bw, bh, bh / 2.0, pc, pc, (ta * 255.0) as u32, 1.0);
                cv.mask(&m.a, m.w, m.h, (bx + 7.0 * sc) as i32, (by + 2.0 * sc) as i32, pm(g.text, (ta * 185.0) as u32));
            }
            cv.reset_clip();

            // items, clipped to the content area and the animated rect
            let top = (py0 + l.body as f32) as i32;
            let bot = (py0 + (l.body + l.rows_vis * l.cell_h) as f32) as i32;
            cv.set_clip((rx as i32).max(0), top.max(ry as i32), (rx + rw) as i32, bot.min((ry + rh) as i32));
            let opening = self.target > 0.5;
            let n = self.items.len();
            if n == 0 && ta > 0.0 {
                let f = fonts.get(&g.font, (g.font_size as f32 * sc) as i32, FW_NORMAL);
                let m = text::render(&to_w(crate::i18n::tr("folder.empty")), f, l.pw - l.pad * 2, 2, true);
                cv.mask(&m.a, m.w, m.h, (px0 + l.pad as f32) as i32, top + (l.cell_h - m.h) / 2, pm(g.text, (ta * 150.0) as u32));
            }
            for i in 0..n {
                let (cx, cy) = self.cell(&l, i);
                if cy + l.cell_h < top || cy > bot {
                    continue;
                }
                let k = (i.min(14)) as f32 * 0.03;
                let ia = if self.cfg.mode == Mode::Panel {
                    1.0
                } else if opening {
                    clamp((self.t - 0.08 - k * 0.6) / 0.42, 0.0, 1.0)
                } else {
                    clamp((e - 0.5) / 0.5, 0.0, 1.0)
                };
                if ia <= 0.0 {
                    continue;
                }
                let ie = ease_out_cubic(ia);
                let dy = (1.0 - ie) * 8.0 * sc;
                let a = (ie * 256.0) as u32;
                let hot = self.hover == i as i32 && matches!(self.drag, Drag::None | Drag::Pending(..));
                let dragged = self.drag == Drag::Item(i);
                if hot || self.press == i as i32 {
                    let hc = if self.press == i as i32 { (g.hover & 0x00FF_FFFF) | (((g.hover >> 24) * 2).min(255) << 24) } else { g.hover };
                    cv.rrect(cx as f32 + 3.0 * sc, cy as f32 + 3.0 * sc, l.cell_w as f32 - 6.0 * sc, l.cell_h as f32 - 6.0 * sc, 10.0 * sc, hc, hc, (ie * 255.0) as u32, 1.0);
                }
                let a = if dragged { a / 3 } else { a };
                let isz = l.icon as f32 * if hot { 1.06 } else { 1.0 };
                // whole pixels keep icons on the fast, exact copy path
                let ix = round(cx as f32 + (l.cell_w as f32 - isz) / 2.0);
                let iy = round(cy as f32 + CELL_TOP as f32 * sc - (isz - l.icon as f32) / 2.0 + dy);
                if let Some(k) = self.keys.get(i) {
                    if let Some(px) = self.cache.get(*k, l.icon) {
                        cv.blit(px, l.icon, l.icon, ix, iy, isz, isz, a);
                    }
                }
                let editing = matches!(self.edit, Some((EditTarget::Item(k), _)) if k == i);
                if let Some(m) = self.labels.get(i).filter(|_| !editing) {
                    let tx = cx + (l.cell_w - m.w) / 2;
                    let ty = cy + (CELL_TOP as f32 * sc) as i32 + l.icon + (CELL_GAP as f32 * sc) as i32 + dy as i32;
                    cv.mask(&m.a, m.w, m.h, tx, ty, scale(pm(g.text, 255), a));
                }
            }
            // insertion marker while drag-reordering
            if self.insert_at >= 0 && n > 0 {
                let i = self.insert_at as usize;
                let (cx, cy) = if i < n { self.cell(&l, i) } else {
                    let (x, y) = self.cell(&l, n - 1);
                    (x + l.cell_w, y)
                };
                cv.rrect(cx as f32 - 1.5 * sc, cy as f32 + 8.0 * sc, 3.0 * sc, l.cell_h as f32 - 16.0 * sc, 1.5 * sc, 0xFFFFFFFF, 0xFFFFFFFF, 200, 1.0);
            }
            // scrollbar
            if l.rows_total > l.rows_vis {
                let track = (bot - top) as f32 - 8.0 * sc;
                let frac = l.rows_vis as f32 / l.rows_total as f32;
                let pos = self.scroll / (l.rows_total - l.rows_vis) as f32;
                let hgt = track * frac;
                let sx = px0 + l.pw as f32 - 6.0 * sc;
                let sy = top as f32 + 4.0 * sc + (track - hgt) * pos;
                cv.rrect(sx, sy, 3.0 * sc, hgt, 1.5 * sc, 0x60FFFFFF, 0x60FFFFFF, (ta * 255.0) as u32, 1.0);
            }
            cv.reset_clip();
            // settings button, fading in when the mouse is near the header
            if self.gear_a > 0.001 && e > 0.9 {
                let (gx, gy, gw, gh) = self.gear_rect(&l);
                let base = if self.gear_hot { 0x2EFFFFFF } else { 0x16FFFFFF };
                cv.rrect(gx, gy, gw, gh, gw / 2.0, base, base, (self.gear_a * 255.0) as u32, 1.0);
                let isz = (16.0 * sc) as i32;
                let m = crate::uikit::gear_mask(isz);
                let c = if self.gear_hot { g.title } else { mix(g.text, 0xFF808080, 0.35) };
                cv.mask(&m, isz, isz, (gx + (gw - isz as f32) / 2.0) as i32, (gy + (gh - isz as f32) / 2.0) as i32, pm(c, (self.gear_a * 255.0) as u32));
            }
            // name being edited in place
            if let Some((t, _)) = self.edit {
                let (fx, fy, fw, fh) = self.edit_rect(g, t);
                let acc = self.cfg.color | 0xFF00_0000;
                cv.rrect(fx, fy, fw, fh, 7.0 * sc, 0x1AFFFFFF, 0x1AFFFFFF, 255, 1.0);
                cv.rrect_stroke(fx, fy, fw, fh, 7.0 * sc, 1.5 * sc, acc, acc, 230);
                let font = fonts.get(&g.font, self.edit_font_px(g, t), edit_weight(t));
                let pad = self.edit_pad();
                let fg = if t == EditTarget::Title { g.title } else { g.text };
                if let Some((_, ed)) = self.edit.as_mut() {
                    ed.draw(&mut cv, font, (fx + pad, fy, fw - pad * 2.0, fh), fg, (acc & 0x00FF_FFFF) | 0x6000_0000, true);
                }
            }
            // collapse button for panels, next to the settings button
            if self.cfg.mode == Mode::Panel && self.gear_a > 0.001 {
                let (cx, cy, cw, ch) = self.chevron_rect(&l);
                if self.chev_hot {
                    cv.rrect(cx, cy, cw, ch, cw / 2.0, 0x2EFFFFFF, 0x2EFFFFFF, (self.gear_a * 255.0) as u32, 1.0);
                }
                let isz = (16.0 * sc) as i32;
                let ic = if self.cfg.collapsed { crate::uikit::Ic::ChevronDown } else { crate::uikit::Ic::ChevronUp };
                let m = crate::uikit::icon_mask_pub(ic, isz);
                let c = if self.chev_hot { g.title } else { mix(g.text, 0xFF808080, 0.35) };
                cv.mask(&m, isz, isz, (cx + (cw - isz as f32) / 2.0) as i32, (cy + (ch - isz as f32) / 2.0) as i32, pm(c, (self.gear_a * 255.0) as u32));
            }
            // Resize grip: two short diagonal strokes tucked inside the rounded corner.
            // It stays hidden until the pointer gets close to the corner.
            let resizing = matches!(self.drag, Drag::Resize(..));
            if self.cfg.mode == Mode::Panel && !self.cfg.locked && !self.rolled() && (self.grip_near || self.grip_hot || resizing) {
                let s = (14.0 * sc).max(10.0) as i32;
                let inset = (g.radius as f32 * sc) * 0.29 + 4.0 * sc;
                let x0 = (px0 + l.pw as f32 - inset - s as f32) as i32;
                let y0 = (py0 + l.ph as f32 - inset - s as f32) as i32;
                let k = s as f32;
                let mut ph = crate::raster::Path::new(crate::raster::Mat::ID);
                ph.move_to(crate::raster::p(k * 0.15, k * 0.95));
                ph.line_to(crate::raster::p(k * 0.95, k * 0.15));
                ph.move_to(crate::raster::p(k * 0.55, k * 0.95));
                ph.line_to(crate::raster::p(k * 0.95, k * 0.55));
                let st = ph.stroke(1.4 * sc, true);
                let cov = crate::raster::rasterize(&st, s as usize, s as usize);
                let c = if self.grip_hot || resizing { 0xC8FFFFFF } else { 0x5CFFFFFF };
                cv.mask(&cov.a, s, s, x0, y0, pm(c, 255));
            }
            if opening && self.cfg.mode == Mode::Hover {
                // keep the whole target area hit-testable while open
                let (a, b) = (self.panel.left - self.win.left, self.panel.top - self.win.top);
                cv.hit_fill(a, b, a + l.pw, b + l.ph);
                let (c, d) = (self.icon_box.left - self.win.left, self.icon_box.top - self.win.top);
                cv.hit_fill(c, d, c + l.box_w, d + l.box_h);
            }
        }

        // ---- Folder glyph. It travels from the icon box into the header.
        if self.cfg.mode == Mode::Hover || e > 0.0 {
            let (gx, gy, gsz) = self.glyph_pos(&l, e);
            self.draw_glyph(&mut cv, gx, gy, gsz, 256, &l);
        }
        if self.cfg.mode == Mode::Hover && e < 0.001 {
            // When collapsed, make the whole icon box clickable because text has gaps
            cv.hit_fill((bx + (bw - l.gs as f32) / 2.0) as i32, by as i32, (bx + (bw + l.gs as f32) / 2.0) as i32, (by + l.box_h as f32) as i32);
        }
        self.canvas = Some(cv);
    }

    fn present(&self, cv: &Canvas) {
        unsafe {
            let pos = POINT { x: self.win.left, y: self.win.top };
            let size = SIZE { cx: cv.w, cy: cv.h };
            let src = POINT { x: 0, y: 0 };
            let bf = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA };
            UpdateLayeredWindow(self.hwnd, 0, &pos, &size, cv.dc, &src, 0, &bf, ULW_ALPHA);
        }
    }
}
