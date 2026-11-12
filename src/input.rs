//! Mouse interaction and drag and drop (inside Crow and from Explorer).

use crate::actions::T_CARET;
use crate::app::*;
use crate::config::Mode;
use crate::ffi::*;
use crate::folder::{Drag, EditTarget, Hit};
use crate::gfx::Canvas;
use crate::store;
use crate::util::*;
use alloc::vec::Vec;

pub const T_GHOST: usize = 11;
/// Posted to the host window to handle a drop after Explorer's drag loop ends.
pub const WM_CROW_DROP: u32 = WM_USER + 0x44;

fn track_leave(h: HWND) {
    let mut t = TRACKMOUSEEVENT { cbSize: core::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
    unsafe { TrackMouseEvent(&mut t) };
}

/// A drop from Explorer waiting to be processed.
pub struct PendingDrop {
    pub folder: u32,
    pub at: i32,
    pub files: Vec<WString>,
}

impl App {
    fn screen(&self, i: usize, p: POINT) -> POINT {
        let w = &self.wins[i].win;
        POINT { x: p.x + w.left, y: p.y + w.top }
    }

    fn on_enter(&mut self, i: usize, sp: POINT) {
        let hwnd = self.wins[i].hwnd;
        self.wins[i].mouse_in = true;
        track_leave(hwnd);
        unsafe { KillTimer(hwnd, T_CLOSE) };
        let now = now_ms();
        let f = &mut self.wins[i];
        f.enter_t = now;
        f.last_move = (sp.x, sp.y, now);
        f.speed = 1.0;
        if f.cfg.mode == Mode::Hover && f.target == 0.0 && f.drag == Drag::None {
            // Get icons and text ready while we decide whether the user wants to open it.
            f.prewarm(&self.g, &mut self.fonts);
            unsafe { SetTimer(hwnd, T_HOVER, 15, 0) };
        }
    }

    pub fn on_move(&mut self, i: usize, p: POINT) {
        let sp = self.screen(i, p);
        if !self.wins[i].mouse_in {
            self.on_enter(i, sp);
        }
        {
            // pointer speed in px/ms, smoothed, for hover intent
            let f = &mut self.wins[i];
            let now = now_ms();
            let (lx, ly, lt) = f.last_move;
            let dt = (now - lt).max(1.0) as f32;
            let d = sqrt(((sp.x - lx) * (sp.x - lx) + (sp.y - ly) * (sp.y - ly)) as f32);
            f.speed = f.speed * 0.5 + (d / dt) * 0.5;
            f.last_move = (sp.x, sp.y, now);
        }
        if self.ghost != 0 {
            self.check_ghost();
        }
        let hwnd = self.wins[i].hwnd;
        match self.wins[i].drag {
            Drag::Pending(hit, start) => {
                let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
                if (sp.x - start.x).abs() <= dx && (sp.y - start.y).abs() <= dy {
                    return;
                }
                let f = &mut self.wins[i];
                match hit {
                    Hit::Icon | Hit::Header | Hit::Title if !f.cfg.locked && f.edit.is_none() => {
                        unsafe { KillTimer(hwnd, T_HOVER) };
                        if f.cfg.mode == Mode::Hover {
                            // Like a desktop icon: the folder stays put (closed, if it was
                            // open) and a see-through copy follows the pointer.
                            f.drag = Drag::Place;
                            if f.target > 0.0 {
                                self.collapse(i);
                                self.render(i);
                            }
                            self.make_folder_ghost(i, start);
                            self.move_ghost(sp);
                        } else {
                            // panels move live, like a window dragged by its title bar
                            f.drag = Drag::Move(start, POINT { x: f.cfg.x, y: f.cfg.y });
                        }
                    }
                    Hit::Grip => {
                        let l = f.layout(&self.g);
                        f.drag = Drag::Resize(start, l.pw, l.ph);
                    }
                    Hit::Item(j) if f.edit.is_none() => {
                        f.drag = Drag::Item(j);
                        f.press = -1;
                        self.drag_src = Some((f.cfg.id, j));
                        self.make_ghost(i, j);
                        self.move_ghost(sp);
                        self.render(i);
                    }
                    _ => f.drag = Drag::None,
                }
            }
            Drag::Move(start, orig) => {
                let nx = self.snap(orig.x + sp.x - start.x);
                let ny = self.snap(orig.y + sp.y - start.y);
                let f = &mut self.wins[i];
                if nx != f.cfg.x || ny != f.cfg.y {
                    let (ddx, ddy) = (nx - f.cfg.x, ny - f.cfg.y);
                    f.cfg.x = nx;
                    f.cfg.y = ny;
                    for r in [&mut f.icon_box, &mut f.panel, &mut f.win] {
                        r.left += ddx;
                        r.right += ddx;
                        r.top += ddy;
                        r.bottom += ddy;
                    }
                    unsafe {
                        SetWindowPos(f.hwnd, 0, f.win.left, f.win.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    crate::blur::restack(f);
                }
            }
            Drag::Resize(start, w0, h0) => {
                let f = &mut self.wins[i];
                let l = f.layout(&self.g);
                let w = w0 + sp.x - start.x;
                let h = h0 + sp.y - start.y;
                let cols = ((w - l.pad * 2 + l.cell_w / 2) / l.cell_w).clamp(1, 16);
                let rows = ((h - l.body - l.pad + l.cell_h / 2) / l.cell_h).clamp(1, 12);
                if cols != f.cfg.columns || rows != f.cfg.rows {
                    f.cfg.columns = cols;
                    f.cfg.rows = rows;
                    self.relayout(i);
                }
            }
            Drag::Item(_) => self.drag_item_move(sp),
            Drag::Place => self.move_ghost(sp),
            Drag::EditSelect => {
                let f = &mut self.wins[i];
                if let Some((t, _)) = f.edit {
                    let font = self.fonts.get(&self.g.font, f.edit_font_px(&self.g, t), crate::folder::edit_weight(t));
                    let fx = f.edit_rect(&self.g, t).0 + f.edit_pad();
                    if let Some((_, ed)) = f.edit.as_mut() {
                        ed.caret = ed.hit(font, fx, p.x as f32);
                    }
                    self.render(i);
                }
            }
            Drag::None => {
                let h = self.wins[i].hit(&self.g, p);
                let hv = if let Hit::Item(j) = h { j as i32 } else { -1 };
                let in_header = matches!(h, Hit::Header | Hit::Title | Hit::Gear | Hit::Chevron);
                let gear_hot = h == Hit::Gear;
                let chev_hot = h == Hit::Chevron;
                let grip_hot = h == Hit::Grip;
                let grip_near = self.wins[i].near_grip(&self.g, p);
                let open = self.popup_open_for(self.wins[i].cfg.id);
                let f = &mut self.wins[i];
                let gear_on = in_header || open;
                let changed = hv != f.hover || gear_hot != f.gear_hot || gear_on != f.gear_on
                    || chev_hot != f.chev_hot || grip_hot != f.grip_hot || grip_near != f.grip_near;
                f.hover = hv;
                f.gear_hot = gear_hot;
                f.chev_hot = chev_hot;
                f.grip_hot = grip_hot;
                f.grip_near = grip_near;
                f.gear_on = gear_on;
                if changed && !f.animating() {
                    self.render(i);
                }
            }
        }
    }

    pub fn on_leave(&mut self, i: usize) {
        let open = self.popup_open_for(self.wins[i].cfg.id);
        let f = &mut self.wins[i];
        f.mouse_in = false;
        let hwnd = f.hwnd;
        unsafe { KillTimer(hwnd, T_HOVER) };
        if f.drag != Drag::None {
            return;
        }
        let rerender = f.hover != -1 || f.cfg.mode == Mode::Panel || f.gear_hot;
        f.hover = -1;
        f.gear_hot = false;
        f.chev_hot = false;
        f.grip_hot = false;
        f.grip_near = false;
        f.gear_on = open;
        if f.cfg.mode == Mode::Hover && f.target > 0.0 && !open && f.edit.is_none() {
            unsafe { SetTimer(hwnd, T_CLOSE, self.g.close_delay.max(1), 0) };
        }
        if rerender && !f.animating() {
            self.render(i);
        }
    }

    pub fn on_timer(&mut self, i: usize, id: usize) {
        let hwnd = self.wins[i].hwnd;
        if id == T_CARET {
            if self.wins[i].edit.is_some() {
                self.render(i);
            } else {
                unsafe { KillTimer(hwnd, T_CARET) };
            }
            return;
        }
        unsafe { KillTimer(hwnd, id) };
        match id {
            T_HOVER => {
                // Hover intent: open as soon as the pointer settles on the folder,
                // or after the configured delay at the latest.
                let f = &self.wins[i];
                if !f.mouse_in || f.drag != Drag::None || f.target > 0.0 {
                    return;
                }
                let c = cursor();
                if !in_rect(&f.icon_box, c) {
                    return;
                }
                let waited = (now_ms() - f.enter_t) as f32;
                let still = (now_ms() - f.last_move.2) as f32 > 30.0;
                let slow = f.speed < 0.18 || still;
                if (waited >= 35.0 && slow) || waited >= self.g.hover_delay as f32 {
                    self.expand(i);
                } else {
                    unsafe { SetTimer(hwnd, T_HOVER, 15, 0) };
                }
            }
            T_CLOSE => {
                let open = self.popup_open_for(self.wins[i].cfg.id);
                let f = &self.wins[i];
                let c = cursor();
                let inside = in_rect(&f.panel, c) || in_rect(&f.icon_box, c);
                if !inside && f.drag == Drag::None && !open && f.edit.is_none() && self.drag_src.is_none() {
                    self.collapse(i);
                } else if !inside {
                    unsafe { SetTimer(hwnd, T_CLOSE, self.g.close_delay.max(1), 0) };
                }
            }
            T_SPRING => {
                if self.spring == self.wins[i].cfg.id {
                    // dragging something over a rolled up panel opens it
                    if self.wins[i].cfg.mode == Mode::Panel && self.wins[i].cfg.collapsed {
                        self.toggle_collapse(i);
                    } else {
                        self.expand(i);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_down(&mut self, i: usize, p: POINT) {
        let sp = self.screen(i, p);
        let h = self.wins[i].hit(&self.g, p);
        // Folder windows never take activation, so a click on them has to close
        // an open popup by hand (the gear button toggles it instead).
        if self.popup.is_some() && h != Hit::Gear {
            self.close_popup(true);
        }
        // while renaming, a click inside the field moves the caret, anywhere else saves
        if let Some((t, _)) = self.wins[i].edit {
            let (fx, fy, fw, fh) = self.wins[i].edit_rect(&self.g, t);
            let (x, y) = (p.x as f32, p.y as f32);
            if x >= fx && x < fx + fw && y >= fy && y < fy + fh {
                let f = &mut self.wins[i];
                let font = self.fonts.get(&self.g.font, f.edit_font_px(&self.g, t), crate::folder::edit_weight(t));
                let pad = f.edit_pad();
                if let Some((_, ed)) = f.edit.as_mut() {
                    let at = ed.hit(font, fx + pad, x);
                    ed.caret = at;
                    if unsafe { GetKeyState(VK_SHIFT) } >= 0 {
                        ed.anchor = at;
                    }
                    ed.stamp = now_ms();
                }
                f.drag = Drag::EditSelect;
                unsafe { SetCapture(f.hwnd) };
                self.render(i);
                return;
            }
            self.commit_edit(i, true);
        }
        let f = &mut self.wins[i];
        f.drag = Drag::Pending(h, sp);
        if let Hit::Item(j) = h {
            f.press = j as i32;
        }
        unsafe { SetCapture(f.hwnd) };
        if f.press >= 0 {
            self.render(i);
        }
    }

    pub fn on_up(&mut self, i: usize, p: POINT) {
        let sp = self.screen(i, p);
        let drag = self.wins[i].drag;
        self.wins[i].drag = Drag::None;
        let press = self.wins[i].press;
        self.wins[i].press = -1;
        unsafe { ReleaseCapture() };
        match drag {
            Drag::Pending(hit, _) => {
                let now = self.wins[i].hit(&self.g, p);
                match (hit, now) {
                    (Hit::Item(a), Hit::Item(b)) if a == b && press == a as i32 => {
                        let path = self.wins[i].item_path(a);
                        store::launch(&path);
                        self.collapse(i);
                    }
                    (Hit::Icon, Hit::Icon) => {
                        if self.wins[i].target == 0.0 {
                            unsafe { KillTimer(self.wins[i].hwnd, T_HOVER) };
                            self.expand(i);
                        } else if self.wins[i].cfg.mode == Mode::Hover {
                            self.collapse(i);
                        }
                    }
                    (Hit::Title, Hit::Title) => {
                        self.start_edit(i, EditTarget::Title, Some(p.x));
                        return;
                    }
                    (Hit::Chevron, Hit::Chevron) => self.toggle_collapse(i),
                    (Hit::Gear, Hit::Gear) => {
                        if self.popup_open_for(self.wins[i].cfg.id) {
                            self.close_popup(true);
                        } else {
                            self.open_settings(i);
                        }
                    }
                    _ => {}
                }
                self.render(i);
            }
            Drag::Move(..) | Drag::Resize(..) => self.save(),
            Drag::Place => {
                let at = POINT { x: sp.x + self.ghost_off.x, y: sp.y + self.ghost_off.y };
                self.destroy_ghost();
                self.drop_folder(i, at);
                // The pointer now rests on the folder it just dropped. That is not
                // a visit meant to open it, so only a fresh one will.
                let f = &mut self.wins[i];
                if in_rect(&f.icon_box, cursor()) {
                    f.mouse_in = true;
                    track_leave(f.hwnd);
                }
            }
            Drag::Item(j) => self.drag_item_drop(i, j, sp),
            Drag::EditSelect | Drag::None => {}
        }
        self.close_if_abandoned();
    }

    /// A hover folder that is open while the pointer is elsewhere (for example
    /// after a drag ended outside it) closes after the usual delay.
    pub fn close_if_abandoned(&mut self) {
        let c = cursor();
        for k in 0..self.wins.len() {
            let open = self.popup_open_for(self.wins[k].cfg.id);
            let f = &self.wins[k];
            if f.cfg.mode == Mode::Hover && f.target > 0.0 && f.edit.is_none() && !open && f.drag == Drag::None
                && !in_rect(&f.panel, c) && !in_rect(&f.icon_box, c)
            {
                unsafe { SetTimer(f.hwnd, T_CLOSE, self.g.close_delay.max(1), 0) };
            }
        }
    }

    pub fn on_dbl(&mut self, i: usize, p: POINT) {
        if let Some((t, _)) = self.wins[i].edit {
            let (fx, ..) = self.wins[i].edit_rect(&self.g, t);
            let f = &mut self.wins[i];
            let font = self.fonts.get(&self.g.font, f.edit_font_px(&self.g, t), crate::folder::edit_weight(t));
            let pad = f.edit_pad();
            if let Some((_, ed)) = f.edit.as_mut() {
                let at = ed.hit(font, fx + pad, p.x as f32);
                ed.select_word(at);
            }
            self.render(i);
            return;
        }
        // double-clicking a panel header rolls it up or down
        if self.wins[i].cfg.mode == Mode::Panel && self.wins[i].hit(&self.g, p) == Hit::Header {
            self.toggle_collapse(i);
            return;
        }
        self.on_down(i, p);
    }

    pub fn on_right(&mut self, i: usize, p: POINT) {
        if self.wins[i].edit.is_some() {
            self.commit_edit(i, true);
        }
        let item = match self.wins[i].hit(&self.g, p) {
            Hit::Item(j) => Some(j),
            _ => None,
        };
        let sp = self.screen(i, p);
        self.open_menu(i, item, sp);
    }

    pub fn on_capture_lost(&mut self, i: usize) {
        // Mouse capture was lost, for example by Alt+Tab during a drag, so cancel cleanly
        match self.wins[i].drag {
            Drag::Item(_) => {
                self.wins[i].drag = Drag::None;
                self.end_item_drag();
                self.render(i);
            }
            Drag::Move(..) | Drag::Resize(..) => {
                self.wins[i].drag = Drag::None;
                self.save();
            }
            Drag::Place => {
                self.wins[i].drag = Drag::None;
                self.destroy_ghost();
            }
            Drag::EditSelect => self.wins[i].drag = Drag::None,
            _ => {}
        }
        self.close_if_abandoned();
    }

    pub fn on_wheel(&mut self, i: usize, delta: i32) {
        let f = &mut self.wins[i];
        if !f.expanded() {
            return;
        }
        let l = f.layout(&self.g);
        let max = (l.rows_total - l.rows_vis).max(0) as f32;
        let t = clamp(f.scroll_target - delta as f32 / 120.0, 0.0, max);
        if t != f.scroll_target {
            f.scroll_target = round(t);
            f.hover = -1;
        }
    }

    pub fn on_cursor(&mut self, i: usize) -> bool {
        let f = &self.wins[i];
        let c = cursor();
        let p = POINT { x: c.x - f.win.left, y: c.y - f.win.top };
        if let Some((t, _)) = f.edit {
            let (fx, fy, fw, fh) = f.edit_rect(&self.g, t);
            let (x, y) = (p.x as f32, p.y as f32);
            if x >= fx && x < fx + fw && y >= fy && y < fy + fh {
                unsafe { SetCursor(LoadCursorW(0, IDC_IBEAM)) };
                return true;
            }
        }
        let id = match f.drag {
            Drag::Move(..) => IDC_SIZEALL,
            Drag::Place => IDC_ARROW,
            Drag::Resize(..) => IDC_SIZENWSE,
            Drag::EditSelect => IDC_IBEAM,
            _ => match f.hit(&self.g, p) {
                Hit::Item(_) | Hit::Gear | Hit::Chevron => IDC_HAND,
                Hit::Title => IDC_IBEAM,
                Hit::Grip if !f.cfg.locked => IDC_SIZENWSE,
                _ => IDC_ARROW,
            },
        };
        unsafe { SetCursor(LoadCursorW(0, id)) };
        true
    }

    pub fn on_dpi(&mut self, i: usize, dpi: u32) {
        let f = &mut self.wins[i];
        f.sc = dpi.max(96) as f32 / 96.0;
        f.invalidate_content();
        f.place(&self.g);
        self.render(i);
    }

    // ------------------------------------------------------ item drag
    fn make_ghost(&mut self, i: usize, j: usize) {
        self.destroy_ghost();
        let Some((px, s)) = self.wins[i].icon_copy(&self.g, j) else { return };
        let sc = self.wins[i].sc;
        let pad = (12.0 * sc) as i32;
        let size = (s as f32 * 1.12) as i32;
        let dim = size + pad * 2;
        let Some(mut cv) = Canvas::dib(dim, dim) else { return };
        self.ghost_off = POINT { x: -dim / 2, y: -dim / 2 };
        self.ghost_alpha = 255;
        cv.clear(0);
        cv.rrect(pad as f32, pad as f32 + 4.0 * sc, size as f32, size as f32, size as f32 * 0.25, 0x60000000, 0x60000000, 255, 10.0 * sc);
        cv.blit(&px, s, s, pad as f32, pad as f32, size as f32, size as f32, 235);
        unsafe {
            let inst = GetModuleHandleW(core::ptr::null());
            self.ghost = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                w!("CrowFolder").as_ptr(), core::ptr::null(), WS_POPUP, 0, 0, dim, dim, 0, 0, inst, core::ptr::null());
            // safety net: the ghost never outlives the mouse button
            SetTimer(self.msg, T_GHOST, 100, 0);
        }
        self.ghost_cv = Some(cv);
    }

    /// The see-through copy of a folder icon that follows the pointer while it
    /// is moved. It keeps the spot where the icon was grabbed under the
    /// pointer, or centers on it when the drag started on the open panel.
    fn make_folder_ghost(&mut self, i: usize, start: POINT) {
        self.destroy_ghost();
        let Some(cv) = self.wins[i].ghost(&self.g, &mut self.fonts) else { return };
        let b = self.wins[i].icon_box;
        self.ghost_off = if in_rect(&b, start) {
            POINT { x: b.left - start.x, y: b.top - start.y }
        } else {
            POINT { x: -cv.w / 2, y: -cv.h / 2 }
        };
        self.ghost_alpha = 150;
        unsafe {
            let inst = GetModuleHandleW(core::ptr::null());
            self.ghost = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                w!("CrowFolder").as_ptr(), core::ptr::null(), WS_POPUP, 0, 0, cv.w, cv.h, 0, 0, inst, core::ptr::null());
            SetTimer(self.msg, T_GHOST, 100, 0);
        }
        self.ghost_cv = Some(cv);
    }

    /// Puts a moved folder icon where it was dropped. When Windows aligns the
    /// desktop icons to a grid, the folder takes the nearest cell of that same
    /// grid so it lines up with the icons around it. Holding Shift places it freely.
    fn drop_folder(&mut self, i: usize, at: POINT) {
        let (x, y) = self.snap_icon(i, at);
        let f = &mut self.wins[i];
        if (x, y) == (f.cfg.x, f.cfg.y) {
            return;
        }
        f.cfg.x = x;
        f.cfg.y = y;
        f.place(&self.g);
        self.render(i);
        self.save();
    }

    fn snap_icon(&self, i: usize, at: POINT) -> (i32, i32) {
        if unsafe { GetKeyState(VK_SHIFT) } < 0 {
            return (at.x, at.y);
        }
        let Some((cw, ch)) = crate::desk::icon_grid() else { return (self.snap(at.x), self.snap(at.y)) };
        let b = &self.wins[i].icon_box;
        let (bw, bh) = (b.right - b.left, b.bottom - b.top);
        let mut mi = MONITORINFO { cbSize: core::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe { GetMonitorInfoW(MonitorFromPoint(POINT { x: at.x + bw / 2, y: at.y + bh / 2 }, MONITOR_DEFAULTTONEAREST), &mut mi) };
        let wa = mi.rcWork;
        // nearest cell by the icon's center across, and by its top edge down
        let col = round((at.x + bw / 2 - wa.left - cw / 2) as f32 / cw as f32).max(0.0) as i32;
        let row = round((at.y - wa.top) as f32 / ch as f32).max(0.0) as i32;
        (wa.left + col * cw + cw / 2 - bw / 2, wa.top + row * ch)
    }

    fn destroy_ghost(&mut self) {
        if self.ghost != 0 {
            unsafe {
                DestroyWindow(self.ghost);
                KillTimer(self.msg, T_GHOST);
            }
            self.ghost = 0;
        }
        self.ghost_cv = None;
    }

    /// Cancels a drag whose mouse button is no longer held, wherever it was released.
    pub fn check_ghost(&mut self) {
        if self.ghost == 0 {
            unsafe { KillTimer(self.msg, T_GHOST) };
            return;
        }
        // Esc cancels a drag, as it does on the desktop
        let held = unsafe { GetAsyncKeyState(VK_LBUTTON) } as u16 & 0x8000 != 0;
        let esc = unsafe { GetAsyncKeyState(VK_ESCAPE as i32) } as u16 & 0x8000 != 0;
        if held && !esc {
            return;
        }
        for k in 0..self.wins.len() {
            if matches!(self.wins[k].drag, Drag::Item(_) | Drag::Place) {
                self.wins[k].drag = Drag::None;
                unsafe { ReleaseCapture() };
                self.render(k);
            }
        }
        self.end_item_drag();
    }

    fn move_ghost(&mut self, sp: POINT) {
        let Some(cv) = &self.ghost_cv else { return };
        unsafe {
            let pos = POINT { x: sp.x + self.ghost_off.x, y: sp.y + self.ghost_off.y };
            let size = SIZE { cx: cv.w, cy: cv.h };
            let src = POINT::default();
            let bf = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: self.ghost_alpha, AlphaFormat: AC_SRC_ALPHA };
            UpdateLayeredWindow(self.ghost, 0, &pos, &size, cv.dc, &src, 0, &bf, ULW_ALPHA);
            ShowWindow(self.ghost, SW_SHOWNOACTIVATE);
        }
    }

    /// Updates drop-target highlighting for whatever is under `sp`.
    fn mark_target(&mut self, sp: POINT, src: Option<usize>) -> Option<usize> {
        let t = self.folder_at(sp);
        for k in 0..self.wins.len() {
            let w = &mut self.wins[k];
            let is_t = Some(k) == t;
            let open = w.expanded() && ((w.cfg.mode == Mode::Panel && !w.rolled()) || w.target > 0.5);
            let dh = is_t && !open && Some(k) != src;
            let ins = if is_t && open && in_rect(&w.panel, sp) {
                let l = w.layout(&self.g);
                let rel = POINT { x: sp.x - w.win.left, y: sp.y - w.win.top };
                w.item_at(&l, rel, true).map_or(-1, |v| v as i32)
            } else {
                -1
            };
            if dh != w.drop_hover || ins != w.insert_at {
                if dh && !w.drop_hover && (w.cfg.mode == Mode::Hover || w.cfg.collapsed) {
                    self.spring = w.cfg.id;
                    unsafe { SetTimer(w.hwnd, T_SPRING, 550, 0) };
                }
                w.drop_hover = dh;
                w.insert_at = ins;
                if !w.animating() {
                    self.render(k);
                }
            }
        }
        if t.is_none() || t.map(|k| !self.wins[k].drop_hover).unwrap_or(true) {
            if t.map(|k| self.wins[k].cfg.id) != Some(self.spring) {
                self.spring = 0;
            }
        }
        t
    }

    fn drag_item_move(&mut self, sp: POINT) {
        self.move_ghost(sp);
        let src = self.drag_src.and_then(|(id, _)| self.by_id(id));
        self.mark_target(sp, src);
    }

    fn end_item_drag(&mut self) {
        self.destroy_ghost();
        self.drag_src = None;
        self.spring = 0;
        self.clear_drop_marks();
    }

    fn drag_item_drop(&mut self, i: usize, j: usize, sp: POINT) {
        let t = self.folder_at(sp);
        let ins = t.map(|k| self.wins[k].insert_at).unwrap_or(-1);
        self.end_item_drag();
        if j >= self.wins[i].items.len() {
            return;
        }
        match t {
            Some(k) if k == i => {
                if ins >= 0 {
                    let f = &mut self.wins[i];
                    let it = f.items.remove(j);
                    let mut at = ins as usize;
                    if at > j {
                        at -= 1;
                    }
                    f.items.insert(at.min(f.items.len()), it);
                    f.sync_order();
                    f.invalidate_content();
                }
            }
            Some(k) => {
                let path = self.wins[i].item_path(j);
                let dir = self.wins[k].dir.clone();
                if let Some(name) = store::move_between(&path, &dir) {
                    self.wins[i].items.remove(j);
                    self.wins[i].sync_order();
                    self.wins[i].invalidate_content();
                    let f = &mut self.wins[k];
                    let at = if ins >= 0 { (ins as usize).min(f.items.len()) } else { f.items.len() };
                    f.items.insert(at, name);
                    f.sync_order();
                    f.invalidate_content();
                    f.place(&self.g);
                    self.render(k);
                }
            }
            None => {
                let path = self.wins[i].item_path(j);
                if store::export_to_desktop(&path) {
                    self.wins[i].items.remove(j);
                    self.wins[i].sync_order();
                    self.wins[i].invalidate_content();
                }
            }
        }
        self.wins[i].place(&self.g);
        self.render(i);
        self.save();
    }

    // ------------------------------------------------------ OLE (Explorer)
    pub fn ole_enter(&mut self, files: Vec<WString>, pt: POINT, allowed: u32) -> u32 {
        self.ole_files = files;
        self.ole_over(pt, allowed)
    }

    pub fn ole_over(&mut self, pt: POINT, allowed: u32) -> u32 {
        if self.ole_files.is_empty() {
            return DROPEFFECT_NONE;
        }
        if self.mark_target(pt, None).is_none() {
            return DROPEFFECT_NONE;
        }
        let want = if self.ole_files.iter().all(|f| store::moves_in(f)) { DROPEFFECT_MOVE } else { DROPEFFECT_LINK };
        for e in [want, DROPEFFECT_MOVE, DROPEFFECT_COPY, DROPEFFECT_LINK] {
            if allowed & e != 0 {
                return e;
            }
        }
        DROPEFFECT_NONE
    }

    pub fn ole_leave(&mut self) {
        self.ole_files.clear();
        self.spring = 0;
        self.clear_drop_marks();
        self.schedule_close_all();
    }

    /// Records the drop and returns right away so Explorer's drag loop (and
    /// its drag image) ends immediately. The files are moved a moment later.
    pub fn ole_drop(&mut self, pt: POINT) -> u32 {
        let files = core::mem::take(&mut self.ole_files);
        let t = self.folder_at(pt);
        let at = t.map(|k| self.wins[k].insert_at).unwrap_or(-1);
        self.spring = 0;
        self.clear_drop_marks();
        let Some(k) = t else { return DROPEFFECT_NONE };
        self.pending_drop = Some(PendingDrop { folder: self.wins[k].cfg.id, at, files });
        unsafe { PostMessageW(self.msg, WM_CROW_DROP, 0, 0) };
        DROPEFFECT_NONE
    }

    /// Moves dropped shortcuts in. Shortcuts on the shared (public) desktop can
    /// be moved once the installer has granted this user access to it.
    pub fn process_drop(&mut self) {
        let Some(d) = self.pending_drop.take() else { return };
        let Some(k) = self.by_id(d.folder) else { return };
        let dir = self.wins[k].dir.clone();
        let mut at = if d.at >= 0 { d.at as usize } else { self.wins[k].items.len() };
        let mut blocked: Vec<WString> = Vec::new();
        let mut names: Vec<WString> = Vec::new();
        for f in &d.files {
            match store::import(&dir, f) {
                store::Import::Done(name) => names.push(name),
                store::Import::NeedsAdmin => blocked.push(f.clone()),
                store::Import::Failed => {}
            }
        }
        // Without permission on the shared desktop (the installer grants it),
        // leave the item there and keep it reachable from the folder.
        for f in &blocked {
            if let Some(name) = store::fallback_in(&dir, f) {
                names.push(name);
            }
        }
        let w = &mut self.wins[k];
        for name in names {
            if let Some(old) = w.items.iter().position(|x| weq_ic(x, &name)) {
                w.items.remove(old);
                if old < at {
                    at -= 1;
                }
            }
            let a = at.min(w.items.len());
            w.items.insert(a, name);
            at = a + 1;
        }
        w.sync_order();
        w.invalidate_content();
        w.place(&self.g);
        self.render(k);
        self.save();
        self.schedule_close_all();
    }

    /// Hover folders opened by a drag close again unless the mouse is in them.
    fn schedule_close_all(&mut self) {
        for w in &self.wins {
            if w.cfg.mode == Mode::Hover && w.target > 0.0 && !w.mouse_in {
                unsafe { SetTimer(w.hwnd, T_CLOSE, self.g.close_delay.max(1), 0) };
            }
        }
    }
}
