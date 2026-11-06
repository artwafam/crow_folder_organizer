//! GDI text rendered once into 8-bit coverage masks (GDI can't write alpha,
//! so we draw white-on-black and use the green channel as coverage).

use crate::ffi::*;
use crate::gfx::Canvas;
use crate::util::*;
use alloc::vec;
use alloc::vec::Vec;

pub struct Mask {
    pub w: i32,
    pub h: i32,
    pub a: Vec<u8>,
}

pub fn font(face: &str, px: i32, weight: i32) -> HFONT {
    let f = wz(&to_w(face));
    unsafe { CreateFontW(-px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, 0, 0, ANTIALIASED_QUALITY, 0, f.as_ptr()) }
}

/// DrawTextW reads the first character even when the length is 0, and an
/// empty Rust slice doesn't point at real memory. That crashed the uninstall
/// window, so empty text gets a real empty string here.
fn ptr(s: &[u16]) -> *const u16 {
    if s.is_empty() { w!("").as_ptr() } else { s.as_ptr() }
}

/// Renders `s` wrapped into at most `lines` lines of width `max_w`.
pub fn render(s: &[u16], f: HFONT, max_w: i32, lines: i32, center: bool) -> Mask {
    let s = trim_nul(s);
    unsafe {
        let dc = CreateCompatibleDC(0);
        let old = SelectObject(dc, f);
        let mut lr = RECT { left: 0, top: 0, right: 1000, bottom: 0 };
        DrawTextW(dc, w!("Ag").as_ptr(), 2, &mut lr, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
        let lh = (lr.bottom - lr.top).max(1);
        let mut flags = DT_NOPREFIX | DT_END_ELLIPSIS | if center { DT_CENTER } else { DT_LEFT };
        if lines > 1 {
            flags |= DT_WORDBREAK | DT_EDITCONTROL;
        } else {
            flags |= DT_SINGLELINE;
        }
        let mut r = RECT { left: 0, top: 0, right: max_w, bottom: lh * lines };
        DrawTextW(dc, ptr(s), s.len() as i32, &mut r, flags | DT_CALCRECT);
        let h = (r.bottom - r.top).clamp(lh, lh * lines);
        SelectObject(dc, old);
        DeleteDC(dc);
        // single-line, left-aligned text gets a tight width (titles, badges)
        let w = if lines == 1 && !center { (r.right - r.left + 2).clamp(1, max_w.max(1)) } else { max_w.max(1) };
        let Some(cv) = Canvas::dib(w, h) else {
            return Mask { w: 0, h: 0, a: Vec::new() };
        };
        let old = SelectObject(cv.dc, f);
        SetBkMode(cv.dc, TRANSPARENT);
        SetTextColor(cv.dc, 0x00FF_FFFF);
        let mut rr = RECT { left: 0, top: 0, right: w, bottom: h };
        DrawTextW(cv.dc, ptr(s), s.len() as i32, &mut rr, flags);
        GdiFlush();
        SelectObject(cv.dc, old);
        let px = core::slice::from_raw_parts(cv.px, (w * h) as usize);
        let a = px.iter().map(|&c| ((c >> 8) & 255) as u8).collect();
        Mask { w, h, a }
    }
}

/// Left aligned text wrapped to `max_w` (wrapping always on, up to `lines`).
pub fn render_left(s: &[u16], f: HFONT, max_w: i32, lines: i32) -> Mask {
    render(s, f, max_w, lines.max(2), false)
}

/// Returns how many lines (1 or 2) the longest label needs at `max_w`, and the line height.
pub fn measure(items: &[WString], face: &str, px: i32, max_w: i32) -> (i32, i32) {
    unsafe {
        let f = font(face, px, FW_NORMAL);
        let dc = CreateCompatibleDC(0);
        let old = SelectObject(dc, f);
        let mut lr = RECT { left: 0, top: 0, right: 1000, bottom: 0 };
        DrawTextW(dc, w!("Ag").as_ptr(), 2, &mut lr, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
        let lh = (lr.bottom - lr.top).max(1);
        let mut lines = 1;
        for it in items {
            let s = stem(it);
            let mut r = RECT { left: 0, top: 0, right: max_w, bottom: 0 };
            DrawTextW(dc, ptr(s), s.len() as i32, &mut r, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX | DT_CENTER);
            if r.bottom - r.top > lh + lh / 2 {
                lines = 2;
                break;
            }
        }
        SelectObject(dc, old);
        DeleteDC(dc);
        DeleteObject(f);
        (lines, lh)
    }
}

/// Soft shadow from a mask (box blur, radius 1, padded by 2px each side).
pub fn shadow(m: &Mask) -> Mask {
    let pad = 2;
    let (w, h) = (m.w + pad * 2, m.h + pad * 2);
    let mut src = vec![0u16; (w * h) as usize];
    for y in 0..m.h {
        for x in 0..m.w {
            src[((y + pad) * w + x + pad) as usize] = m.a[(y * m.w + x) as usize] as u16;
        }
    }
    let mut tmp = vec![0u16; (w * h) as usize];
    for _ in 0..2 {
        for y in 0..h {
            for x in 0..w {
                let mut s = 0u32;
                for dx in -1..=1 {
                    let xx = (x + dx).clamp(0, w - 1);
                    s += src[(y * w + xx) as usize] as u32;
                }
                tmp[(y * w + x) as usize] = (s / 3) as u16;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut s = 0u32;
                for dy in -1..=1 {
                    let yy = (y + dy).clamp(0, h - 1);
                    s += tmp[(yy * w + x) as usize] as u32;
                }
                src[(y * w + x) as usize] = (s / 3) as u16;
            }
        }
    }
    Mask { w, h, a: src.iter().map(|&v| (v.min(255) as u32 * 3 / 2).min(255) as u8).collect() }
}
