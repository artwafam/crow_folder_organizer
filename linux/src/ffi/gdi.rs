//! The GDI subset Crow uses: memory DCs, 32-bit DIB sections and text.
//! Text is drawn the way Crow expects from GDI: the text color over what is
//! already in the bitmap, alpha untouched.

use super::font::Font;
use super::*;
use std::collections::HashMap;

enum Obj {
    Dc { bmp: HGDIOBJ, font: HGDIOBJ, color: u32 },
    Bitmap { w: i32, h: i32, px: Vec<u32> },
    Font(Font),
}

struct Gdi {
    objs: HashMap<isize, Obj>,
    next: isize,
}

static mut G: Option<Gdi> = None;

fn g() -> &'static mut Gdi {
    unsafe { (*core::ptr::addr_of_mut!(G)).get_or_insert_with(|| Gdi { objs: HashMap::new(), next: 0x2_0000 }) }
}

fn add(o: Obj) -> isize {
    let g = g();
    g.next += 4;
    g.objs.insert(g.next, o);
    g.next
}

const STOCK: isize = 0x7FFF_0000;

/// The pixels of the bitmap selected into `dc`: (pointer, width, height).
pub fn dc_pixels(dc: HDC) -> Option<(*const u32, i32, i32)> {
    let g = g();
    let Some(Obj::Dc { bmp, .. }) = g.objs.get(&dc) else { return None };
    match g.objs.get(bmp) {
        Some(Obj::Bitmap { w, h, px }) => Some((px.as_ptr(), *w, *h)),
        _ => None,
    }
}

pub unsafe extern "system" fn CreateCompatibleDC(_dc: HDC) -> HDC {
    add(Obj::Dc { bmp: 0, font: 0, color: 0 })
}

pub unsafe extern "system" fn DeleteDC(dc: HDC) -> BOOL {
    matches!(g().objs.remove(&dc), Some(Obj::Dc { .. })) as BOOL
}

pub unsafe extern "system" fn CreateDIBSection(_dc: HDC, bi: *const BITMAPINFO, _usage: u32, bits: *mut *mut c_void, _sec: HANDLE, _off: u32) -> HBITMAP {
    let h = &(*bi).bmiHeader;
    let (w, ht) = (h.biWidth, h.biHeight.abs());
    if w <= 0 || ht <= 0 || h.biBitCount != 32 || (w as i64 * ht as i64) > (1 << 28) {
        return 0;
    }
    let mut px = vec![0u32; (w * ht) as usize];
    *bits = px.as_mut_ptr() as *mut c_void;
    add(Obj::Bitmap { w, h: ht, px })
}

/// Creates a bitmap from pixels (used by the Linux image loader).
pub fn bitmap_from(w: i32, h: i32, px: Vec<u32>) -> HBITMAP {
    add(Obj::Bitmap { w, h, px })
}

pub unsafe extern "system" fn SelectObject(dc: HDC, o: HGDIOBJ) -> HGDIOBJ {
    let g = g();
    let kind = match g.objs.get(&o) {
        Some(Obj::Bitmap { .. }) => 1,
        Some(Obj::Font(_)) => 2,
        _ if o == 0 => 0,
        _ => return 0,
    };
    let Some(Obj::Dc { bmp, font, .. }) = g.objs.get_mut(&dc) else { return 0 };
    match kind {
        1 => core::mem::replace(bmp, o),
        2 => core::mem::replace(font, o),
        _ => 0,
    }
}

pub unsafe extern "system" fn DeleteObject(o: HGDIOBJ) -> BOOL {
    match g().objs.get(&o) {
        Some(Obj::Bitmap { .. }) | Some(Obj::Font(_)) => {
            g().objs.remove(&o);
            1
        }
        _ => 0,
    }
}

pub unsafe extern "system" fn GetStockObject(i: i32) -> HGDIOBJ {
    STOCK + i as isize
}

pub unsafe extern "system" fn GetObjectW(h: HGDIOBJ, n: i32, p: *mut c_void) -> i32 {
    let Some(Obj::Bitmap { w, h: ht, px }) = g().objs.get_mut(&h) else { return 0 };
    if (n as usize) < core::mem::size_of::<BITMAP>() {
        return 0;
    }
    *(p as *mut BITMAP) = BITMAP { bmType: 0, bmWidth: *w, bmHeight: *ht, bmWidthBytes: *w * 4, bmPlanes: 1, bmBitsPixel: 32, bmBits: px.as_mut_ptr() as *mut c_void };
    core::mem::size_of::<BITMAP>() as i32
}

pub unsafe extern "system" fn GetDIBits(_dc: HDC, bm: HBITMAP, start: u32, lines: u32, bits: *mut c_void, _bi: *mut BITMAPINFO, _usage: u32) -> i32 {
    let Some(Obj::Bitmap { w, h, px }) = g().objs.get(&bm) else { return 0 };
    let start = start.min(*h as u32) as usize;
    let n = (lines as usize).min(*h as usize - start);
    let row = *w as usize;
    core::ptr::copy_nonoverlapping(px.as_ptr().add(start * row), bits as *mut u32, n * row);
    n as i32
}

pub unsafe extern "system" fn CreateFontW(h: i32, _w: i32, _esc: i32, _ori: i32, weight: i32, _italic: u32, _underline: u32, _strike: u32,
    _charset: u32, _out: u32, _clip: u32, _quality: u32, _pitch: u32, face: PCWSTR) -> HFONT {
    let face = wstring(face);
    match Font::new(&face, h.abs(), if weight == 0 { 400 } else { weight }) {
        Some(f) => add(Obj::Font(f)),
        None => 0,
    }
}

pub unsafe extern "system" fn SetTextColor(dc: HDC, c: u32) -> u32 {
    match g().objs.get_mut(&dc) {
        Some(Obj::Dc { color, .. }) => core::mem::replace(color, c),
        _ => 0,
    }
}

pub unsafe extern "system" fn SetBkMode(_dc: HDC, _m: i32) -> i32 {
    1
}

pub unsafe extern "system" fn GdiFlush() -> BOOL {
    1
}

fn dc_font(dc: HDC) -> Option<&'static Font> {
    let g = g();
    let Some(Obj::Dc { font, .. }) = g.objs.get(&dc) else { return None };
    match g.objs.get(font) {
        Some(Obj::Font(f)) => Some(f),
        _ => None,
    }
}

fn chars_of(s: PCWSTR, n: i32) -> Vec<char> {
    let w: &[u16] = if s.is_null() || n == 0 {
        &[]
    } else if n < 0 {
        unsafe { core::slice::from_raw_parts(s, (0..).take_while(|&i| *s.add(i) != 0).count()) }
    } else {
        unsafe { core::slice::from_raw_parts(s, n as usize) }
    };
    char::decode_utf16(w.iter().copied()).map(|r| r.unwrap_or('\u{fffd}')).collect()
}

/// Splits text into lines of at most `max_w` pixels, breaking after spaces,
/// between CJK characters, and inside words that are too long by themselves.
fn wrap(f: &Font, s: &[char], max_w: f32) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start <= s.len() {
        let end = s[start..].iter().position(|&c| c == '\n').map_or(s.len(), |p| start + p);
        let para = &s[start..end];
        let adv = f.advances(para);
        let width = |a: usize, b: usize| if b == 0 { 0.0 } else { adv[b - 1] - if a == 0 { 0.0 } else { adv[a - 1] } };
        let mut a = 0;
        while a < para.len() || (a == 0 && para.is_empty()) {
            if para.is_empty() {
                out.push((start, start));
                break;
            }
            let mut b = a;
            let mut last_break = None;
            while b < para.len() && width(a, b + 1) <= max_w {
                b += 1;
                let c = para[b - 1];
                if c == ' ' || c == '\t' || c == '-' || is_cjk(c) || para.get(b).is_some_and(|&n| is_cjk(n)) {
                    last_break = Some(b);
                }
            }
            let cut = if b >= para.len() {
                para.len()
            } else {
                match last_break {
                    Some(k) if k > a => k,
                    _ => b.max(a + 1),
                }
            };
            // trailing spaces don't count on the line
            let mut e = cut;
            while e > a && para[e - 1] == ' ' {
                e -= 1;
            }
            out.push((start + a, start + e));
            a = cut;
            while a < para.len() && para[a] == ' ' {
                a += 1;
            }
        }
        start = end + 1;
    }
    if out.is_empty() {
        out.push((0, 0));
    }
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// `s` cut so that it plus an ellipsis fits in `max_w`.
fn ellipsize(f: &Font, s: &[char], max_w: f32) -> Vec<char> {
    let adv = f.advances(s);
    if adv.last().copied().unwrap_or(0.0) <= max_w {
        return s.to_vec();
    }
    let ell = f.advances(&['…'])[0];
    let mut n = s.len();
    while n > 0 && adv[n - 1] + ell > max_w {
        n -= 1;
    }
    while n > 0 && s[n - 1] == ' ' {
        n -= 1;
    }
    let mut v = s[..n].to_vec();
    v.push('…');
    v
}

pub unsafe extern "system" fn DrawTextW(dc: HDC, s: PCWSTR, n: i32, r: *mut RECT, flags: u32) -> i32 {
    let Some(f) = dc_font(dc) else { return 0 };
    let text: Vec<char> = chars_of(s, n).into_iter().map(|c| if c == '\r' { ' ' } else { c }).collect();
    let rc = &mut *r;
    let lh = f.line_height();
    let max_w = (rc.right - rc.left) as f32;
    let single = flags & DT_SINGLELINE != 0;
    let mut lines: Vec<Vec<char>> = if single {
        vec![text.iter().map(|&c| if c == '\n' { ' ' } else { c }).collect()]
    } else if flags & DT_WORDBREAK != 0 {
        wrap(f, &text, max_w.max(1.0)).into_iter().map(|(a, b)| text[a..b].to_vec()).collect()
    } else {
        text.split(|&c| c == '\n').map(|l| l.to_vec()).collect()
    };
    let widths = |ls: &Vec<Vec<char>>| ls.iter().map(|l| f.advances(l).last().copied().unwrap_or(0.0)).collect::<Vec<f32>>();
    if flags & DT_CALCRECT != 0 {
        let w = widths(&lines).into_iter().fold(0.0f32, f32::max).ceil() as i32;
        let h = lh * lines.len() as i32;
        if single || flags & DT_WORDBREAK == 0 {
            rc.right = rc.left + w;
        } else {
            rc.right = rc.right.max(rc.left + w);
        }
        rc.bottom = rc.top + h;
        return h;
    }
    // Only lines that fit completely are shown, and the last one shown ends
    // with an ellipsis when text was cut.
    let fit = if single { 1 } else { (((rc.bottom - rc.top) / lh.max(1)) as usize).max(1) };
    let ell = flags & (DT_END_ELLIPSIS | DT_WORD_ELLIPSIS) != 0;
    if lines.len() > fit {
        let mut rest: Vec<char> = Vec::new();
        for (k, l) in lines.drain(fit - 1..).enumerate() {
            if k > 0 {
                rest.push(' ');
            }
            rest.extend(l);
        }
        lines.push(if ell { ellipsize(f, &rest, max_w) } else { rest });
    } else if ell {
        for l in lines.iter_mut() {
            *l = ellipsize(f, l, max_w);
        }
    }
    let ws = widths(&lines);
    let Some((bits, bw, bh)) = dc_pixels(dc) else { return lh * lines.len() as i32 };
    let color = match g().objs.get(&dc) {
        Some(Obj::Dc { color, .. }) => *color,
        _ => 0,
    };
    // COLORREF is 0x00BBGGRR
    let (cr, cg, cb) = ((color & 255) as f32, ((color >> 8) & 255) as f32, ((color >> 16) & 255) as f32);
    let px = core::slice::from_raw_parts_mut(bits as *mut u32, (bw * bh) as usize);
    let total = lh * lines.len() as i32;
    let top = if single && flags & DT_VCENTER != 0 { rc.top + (rc.bottom - rc.top - lh) / 2 } else { rc.top };
    let asc = f.ascent();
    let (cx0, cy0, cx1, cy1) = (rc.left.max(0), rc.top.max(0), rc.right.min(bw), rc.bottom.min(bh));
    for (i, l) in lines.iter().enumerate() {
        let x = if flags & DT_CENTER != 0 {
            rc.left as f32 + ((max_w - ws[i]) / 2.0).round()
        } else if flags & 2 != 0 {
            rc.right as f32 - ws[i]
        } else {
            rc.left as f32
        };
        let base = (top + i as i32 * lh) as f32 + asc.round();
        f.draw(l, x, base, &mut |gx, gy, cov| {
            if gx < cx0 || gy < cy0 || gx >= cx1 || gy >= cy1 {
                return;
            }
            let a = cov.clamp(0.0, 1.0);
            let p = &mut px[(gy * bw + gx) as usize];
            let mix = |d: u32, c: f32| (d as f32 * (1.0 - a) + c * a + 0.5) as u32;
            let d = *p;
            *p = (d & 0xFF00_0000) | (mix((d >> 16) & 255, cr) << 16) | (mix((d >> 8) & 255, cg) << 8) | mix(d & 255, cb);
        });
    }
    total
}

pub unsafe extern "system" fn GetTextExtentExPointW(dc: HDC, s: PCWSTR, n: i32, max: i32, fit: *mut i32, dx: *mut i32, sz: *mut SIZE) -> BOOL {
    let Some(f) = dc_font(dc) else { return 0 };
    // one entry per UTF-16 unit, like Windows
    let w = if s.is_null() || n <= 0 { &[][..] } else { core::slice::from_raw_parts(s, n as usize) };
    let chars: Vec<char> = char::decode_utf16(w.iter().copied()).map(|r| r.unwrap_or('\u{fffd}')).collect();
    let adv = f.advances(&chars);
    let mut k = 0usize;
    let mut fits = 0;
    for (ci, c) in chars.iter().enumerate() {
        let v = adv[ci].round() as i32;
        for _ in 0..c.len_utf16() {
            if !dx.is_null() {
                *dx.add(k) = v;
            }
            if v <= max {
                fits = k as i32 + 1;
            }
            k += 1;
        }
    }
    if !fit.is_null() {
        *fit = fits;
    }
    if !sz.is_null() {
        *sz = SIZE { cx: adv.last().map_or(0, |v| v.round() as i32), cy: f.line_height() };
    }
    1
}
