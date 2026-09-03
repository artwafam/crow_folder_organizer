//! Single line text editing, drawn by us. It powers renaming folders and
//! shortcuts right where their names are shown, and the text fields in the
//! settings popover. Supports selection, word jumps, clipboard and undo of the
//! whole edit (Esc).

use crate::ffi::*;
use crate::gfx::{pm, Canvas};
use crate::text;
use crate::util::*;
use alloc::vec::Vec;

pub enum Key {
    None,
    Changed,
    Moved,
    Commit,
    Cancel,
}

pub struct Edit {
    pub text: Vec<u16>,
    pub caret: usize,
    pub anchor: usize,
    /// Horizontal scroll in pixels, so the caret always stays visible.
    pub scroll: f32,
    /// Time of the last input. The caret stays solid for a moment after typing.
    pub stamp: f64,
    pub max: usize,
    /// Characters a file name can't contain are refused when this is set.
    pub file_name: bool,
    /// When the user is dragging a selection with the mouse.
    pub selecting: bool,
}

fn is_word(c: u16) -> bool {
    !(c == b' ' as u16 || c == b'.' as u16 || c == b'-' as u16 || c == b'_' as u16 || c == b'(' as u16 || c == b')' as u16)
}

impl Edit {
    pub fn new(s: &[u16], max: usize, file_name: bool) -> Edit {
        let text: Vec<u16> = trim_nul(s).iter().copied().take(max).collect();
        let n = text.len();
        Edit { text, caret: n, anchor: 0, scroll: 0.0, stamp: now_ms(), max, file_name, selecting: false }
    }

    pub fn sel(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }
    pub fn has_sel(&self) -> bool {
        self.caret != self.anchor
    }
    fn touch(&mut self) {
        self.stamp = now_ms();
    }
    fn delete_sel(&mut self) -> bool {
        let (a, b) = self.sel();
        if a == b {
            return false;
        }
        self.text.drain(a..b);
        self.caret = a;
        self.anchor = a;
        true
    }

    pub fn insert(&mut self, s: &[u16]) {
        self.delete_sel();
        for &c in s {
            if self.text.len() >= self.max {
                break;
            }
            if c < 0x20 || (self.file_name && b"\\/:*?\"<>|".iter().any(|&b| b as u16 == c)) {
                continue;
            }
            self.text.insert(self.caret, c);
            self.caret += 1;
        }
        self.anchor = self.caret;
        self.touch();
    }

    pub fn on_char(&mut self, c: u16) -> Key {
        if c < 0x20 || c == 0x7F {
            return Key::None;
        }
        self.insert(&[c]);
        Key::Changed
    }

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0 && !is_word(self.text[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word(self.text[i - 1]) {
            i -= 1;
        }
        i
    }
    fn word_right(&self, mut i: usize) -> usize {
        let n = self.text.len();
        while i < n && is_word(self.text[i]) {
            i += 1;
        }
        while i < n && !is_word(self.text[i]) {
            i += 1;
        }
        i
    }

    pub fn on_key(&mut self, vk: usize, hwnd: HWND) -> Key {
        let ctrl = unsafe { GetKeyState(VK_CONTROL) } < 0;
        let shift = unsafe { GetKeyState(VK_SHIFT) } < 0;
        let n = self.text.len();
        let moved = |e: &mut Edit, to: usize| {
            e.caret = to.min(e.text.len());
            if !shift {
                e.anchor = e.caret;
            }
            e.touch();
            Key::Moved
        };
        match vk {
            VK_RETURN => Key::Commit,
            VK_ESCAPE => Key::Cancel,
            VK_LEFT => {
                let to = if !shift && self.has_sel() { self.sel().0 } else if ctrl { self.word_left(self.caret) } else { self.caret.saturating_sub(1) };
                moved(self, to)
            }
            VK_RIGHT => {
                let to = if !shift && self.has_sel() { self.sel().1 } else if ctrl { self.word_right(self.caret) } else { self.caret + 1 };
                moved(self, to)
            }
            VK_HOME => moved(self, 0),
            VK_END => moved(self, n),
            VK_BACK => {
                if !self.delete_sel() && self.caret > 0 {
                    let from = if ctrl { self.word_left(self.caret) } else { self.caret - 1 };
                    self.text.drain(from..self.caret);
                    self.caret = from;
                    self.anchor = from;
                }
                self.touch();
                Key::Changed
            }
            VK_DELETE => {
                if !self.delete_sel() && self.caret < n {
                    let to = if ctrl { self.word_right(self.caret) } else { self.caret + 1 };
                    self.text.drain(self.caret..to);
                }
                self.touch();
                Key::Changed
            }
            0x41 if ctrl => {
                // Ctrl+A
                self.anchor = 0;
                self.caret = n;
                Key::Moved
            }
            0x43 | 0x58 if ctrl => {
                // Ctrl+C and Ctrl+X
                let (a, b) = self.sel();
                if a < b {
                    copy_text(hwnd, &self.text[a..b]);
                    if vk == 0x58 {
                        self.delete_sel();
                        self.touch();
                        return Key::Changed;
                    }
                }
                Key::None
            }
            0x56 if ctrl => {
                // Ctrl+V, pasting a single line only
                if let Some(t) = paste_text(hwnd) {
                    let line: Vec<u16> = t.into_iter().take_while(|&c| c != 13 && c != 10).collect();
                    self.insert(&line);
                    return Key::Changed;
                }
                Key::None
            }
            _ => Key::None,
        }
    }

    /// Caret index closest to a pixel offset, given cumulative advances.
    pub fn index_at(adv: &[i32], x: f32) -> usize {
        for i in 1..adv.len() {
            let mid = (adv[i - 1] + adv[i]) as f32 / 2.0;
            if x < mid {
                return i - 1;
            }
        }
        adv.len().saturating_sub(1)
    }

    pub fn select_word(&mut self, at: usize) {
        let at = at.min(self.text.len());
        let mut a = at;
        while a > 0 && is_word(self.text[a - 1]) {
            a -= 1;
        }
        let mut b = at;
        while b < self.text.len() && is_word(self.text[b]) {
            b += 1;
        }
        self.anchor = a;
        self.caret = b;
        self.touch();
    }

    /// Whether the caret should be drawn right now (it blinks).
    pub fn caret_visible(&self) -> bool {
        let blink = unsafe { GetCaretBlinkTime() }.clamp(200, 1200) as f64;
        let t = now_ms() - self.stamp;
        t < blink || ((t / blink) as i64) % 2 == 0
    }

    /// Draws the field contents (selection, text, caret) inside `rect`, keeping
    /// the caret in view. `rect` is (x, y, w, h) in canvas pixels.
    pub fn draw(&mut self, cv: &mut Canvas, f: HFONT, rect: (f32, f32, f32, f32), fg: u32, sel_bg: u32, focused: bool) {
        let (x, y, w, h) = rect;
        let adv = advances(f, &self.text);
        let caret_x = adv[self.caret.min(adv.len() - 1)] as f32;
        // keep the caret inside the visible part, with a little margin
        let margin = 4.0;
        if caret_x - self.scroll > w - margin {
            self.scroll = caret_x - w + margin;
        }
        if caret_x - self.scroll < 0.0 {
            self.scroll = (caret_x - margin).max(0.0);
        }
        let total = *adv.last().unwrap_or(&0) as f32;
        if total < w {
            self.scroll = 0.0;
        }
        let ox = x - self.scroll;
        let old = cv.clip;
        cv.set_clip(x as i32, y as i32, (x + w) as i32 + 1, (y + h) as i32 + 1);
        let m = text::render(&self.text, f, (total as i32 + 4).max(4), 1, false);
        let ty = y + (h - m.h as f32) / 2.0;
        if focused && self.has_sel() {
            let (a, b) = self.sel();
            let (sx, ex) = (adv[a] as f32, adv[b] as f32);
            cv.rrect(ox + sx, ty, ex - sx, m.h as f32, 2.0, sel_bg, sel_bg, 255, 1.0);
        }
        cv.mask(&m.a, m.w, m.h, ox as i32, ty as i32, pm(fg, 255));
        if focused && self.caret_visible() {
            cv.rrect(ox + caret_x - 0.5, ty + 1.0, 1.6, m.h as f32 - 2.0, 0.0, fg, fg, 255, 1.0);
        }
        cv.clip = old;
    }

    /// Caret index for a click at canvas x inside a field starting at `field_x`.
    pub fn hit(&self, f: HFONT, field_x: f32, x: f32) -> usize {
        let adv = advances(f, &self.text);
        Edit::index_at(&adv, x - field_x + self.scroll)
    }
}

/// Cumulative pixel advance for every prefix of `s` (len + 1 entries, first is 0).
pub fn advances(f: HFONT, s: &[u16]) -> Vec<i32> {
    let mut out = alloc::vec![0i32; s.len() + 1];
    if s.is_empty() {
        return out;
    }
    unsafe {
        let dc = CreateCompatibleDC(0);
        let old = SelectObject(dc, f);
        let mut fit = 0;
        let mut sz = SIZE::default();
        GetTextExtentExPointW(dc, s.as_ptr(), s.len() as i32, i32::MAX, &mut fit, out[1..].as_mut_ptr(), &mut sz);
        SelectObject(dc, old);
        DeleteDC(dc);
    }
    out
}

fn copy_text(hwnd: HWND, s: &[u16]) {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, (s.len() + 1) * 2);
        if h != 0 {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
                *p.add(s.len()) = 0;
                GlobalUnlock(h);
                if SetClipboardData(CF_UNICODETEXT, h) == 0 {
                    GlobalFree(h);
                }
            } else {
                GlobalFree(h);
            }
        }
        CloseClipboard();
    }
}

fn paste_text(hwnd: HWND) -> Option<Vec<u16>> {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(CF_UNICODETEXT);
        if h != 0 {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let n = crate::rt::wcslen(p).min(4096);
                out = Some(core::slice::from_raw_parts(p, n).to_vec());
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}
