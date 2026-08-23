//! Premultiplied BGRA canvas and compositing helpers.
//! Pixels are u32 0xAARRGGBB, premultiplied (what UpdateLayeredWindow wants).

use crate::ffi::*;
use crate::raster::Coverage;
use crate::util::*;

/// Converts straight ARGB to premultiplied, applying an extra alpha from 0 to 255.
#[inline]
pub fn pm(c: u32, extra: u32) -> u32 {
    let a = ((c >> 24) * extra + 127) / 255;
    let r = (((c >> 16) & 255) * a + 127) / 255;
    let g = (((c >> 8) & 255) * a + 127) / 255;
    let b = ((c & 255) * a + 127) / 255;
    (a << 24) | (r << 16) | (g << 8) | b
}

/// Scale a premultiplied pixel by k in 0..=256.
#[inline(always)]
pub fn scale(c: u32, k: u32) -> u32 {
    let rb = ((c & 0x00FF_00FF) * k >> 8) & 0x00FF_00FF;
    let ag = (((c >> 8) & 0x00FF_00FF) * k) & 0xFF00_FF00;
    rb | ag
}

/// src over dst, both premultiplied.
#[inline(always)]
pub fn over(d: u32, s: u32) -> u32 {
    let sa = s >> 24;
    if sa == 255 {
        return s;
    }
    if s == 0 {
        return d;
    }
    s + scale(d, 256 - sa - (sa >> 7))
}

/// Lerp between two straight ARGB colors.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = (clamp(t, 0.0, 1.0) * 256.0) as u32;
    let mut out = 0;
    for sh in [0, 8, 16, 24] {
        let x = (a >> sh) & 255;
        let y = (b >> sh) & 255;
        out |= (((x * (256 - t) + y * t) >> 8) & 255) << sh;
    }
    out
}

/// Brighten (k>0) or darken (k<0) a straight color, keeping alpha.
pub fn shade(c: u32, k: f32) -> u32 {
    let target = if k >= 0.0 { 0xFFFF_FFFF } else { 0xFF00_0000 };
    (c & 0xFF00_0000) | (mix(c, target, abs(k)) & 0x00FF_FFFF)
}


pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: *mut u32,
    pub dc: HDC,
    /// Clip rect [x0, y0, x1, y1] applied by all drawing ops.
    pub clip: [i32; 4],
    bmp: HBITMAP,
    old: HGDIOBJ,
    owned: alloc::vec::Vec<u32>,
}

impl Canvas {
    /// GDI-backed canvas (needed for UpdateLayeredWindow and GDI text).
    pub fn dib(w: i32, h: i32) -> Option<Canvas> {
        unsafe {
            let mut bi = BITMAPINFO::default();
            bi.bmiHeader.biSize = core::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            let dc = CreateCompatibleDC(0);
            let mut bits = core::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, 0, 0);
            if bmp == 0 || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bmp);
            Some(Canvas { w, h, px: bits as *mut u32, dc, clip: [0, 0, w, h], bmp, old, owned: alloc::vec::Vec::new() })
        }
    }
    /// Plain heap canvas.
    pub fn mem(w: i32, h: i32) -> Canvas {
        let mut owned = alloc::vec![0u32; (w.max(1) * h.max(1)) as usize];
        let px = owned.as_mut_ptr();
        Canvas { w, h, px, dc: 0, clip: [0, 0, w, h], bmp: 0, old: 0, owned }
    }
    pub fn into_vec(mut self) -> alloc::vec::Vec<u32> {
        core::mem::take(&mut self.owned)
    }
    #[inline(always)]
    pub fn pixels(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.px, (self.w * self.h) as usize) }
    }
    pub fn set_clip(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.clip = [x0.max(0), y0.max(0), x1.min(self.w), y1.min(self.h)];
    }
    pub fn reset_clip(&mut self) {
        self.clip = [0, 0, self.w, self.h];
    }
    pub fn clear(&mut self, c: u32) {
        self.pixels().fill(c);
    }

    /// Fills a rounded rect with optional vertical gradient (straight colors
    /// `top`/`bot`), extra alpha, and edge softness (1 = crisp AA, >1 = blur).
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, top: u32, bot: u32, alpha: u32, soft: f32) {
        self.rrect_impl(x, y, w, h, r, top, bot, alpha, soft, false);
    }

    /// A soft shadow that skips its fully covered middle. Use it under an
    /// opaque surface, where the middle would be painted over anyway.
    pub fn shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: u32, alpha: u32, soft: f32) {
        self.rrect_impl(x, y, w, h, r, c, c, alpha, soft, true);
    }

    fn rrect_impl(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, top: u32, bot: u32, alpha: u32, soft: f32, hollow: bool) {
        if w <= 0.0 || h <= 0.0 || alpha == 0 {
            return;
        }
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let pad = soft;
        let x0 = (floor(x - pad) as i32).max(self.clip[0]);
        let y0 = (floor(y - pad) as i32).max(self.clip[1]);
        let x1 = (ceil(x + w + pad) as i32).min(self.clip[2]);
        let y1 = (ceil(y + h + pad) as i32).min(self.clip[3]);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let (mx, my) = (x + w / 2.0, y + h / 2.0);
        let (ex, ey) = (w / 2.0 - r, h / 2.0 - r);
        let inv_soft = 1.0 / soft;
        let grad = top != bot;
        let mut col = pm(top, alpha);
        let stride = self.w as usize;
        let px = self.pixels();
        for py in y0..y1 {
            if grad {
                col = pm(mix(top, bot, (py as f32 + 0.5 - y) / h), alpha);
            }
            let cy = py as f32 + 0.5;
            let qy = abs(cy - my) - ey;
            // Pixels whose centers sit within `inner` of the middle are fully
            // covered, so they skip the distance math entirely.
            let inner = inner_half(abs(cy - my), w / 2.0, h / 2.0, r, 0.5 * soft);
            let (fa, fb) = if inner >= 0.0 {
                ((ceil(mx - inner - 0.5) as i32).max(x0), (floor(mx + inner - 0.5) as i32 + 1).min(x1))
            } else {
                (x1, x1)
            };
            let row = &mut px[py as usize * stride..(py as usize + 1) * stride];
            let edge = |pxx: i32, row: &mut [u32]| {
                let qx = abs(pxx as f32 + 0.5 - mx) - ex;
                let d = if qx > 0.0 && qy > 0.0 { sqrt(qx * qx + qy * qy) - r } else { qx.max(qy) - r };
                let mut cov = 0.5 - d * inv_soft;
                if cov <= 0.0 {
                    return;
                }
                let dst = &mut row[pxx as usize];
                if cov >= 1.0 {
                    *dst = over(*dst, col);
                } else {
                    if soft > 1.0 {
                        cov = cov * cov * (3.0 - 2.0 * cov);
                    }
                    *dst = over(*dst, scale(col, (cov * 256.0) as u32));
                }
            };
            for pxx in x0..fa.min(x1) {
                edge(pxx, row);
            }
            if fa < fb && !hollow {
                let span = &mut row[fa as usize..fb as usize];
                if col >> 24 == 255 {
                    span.fill(col);
                } else {
                    for d in span.iter_mut() {
                        *d = over(*d, col);
                    }
                }
            }
            for pxx in fb.max(fa).max(x0)..x1 {
                edge(pxx, row);
            }
        }
    }

    /// 1px-ish ring (border) of a rounded rect.
    pub fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, top: u32, bot: u32, alpha: u32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let ri = (r - t).max(0.0);
        let x0 = (floor(x) as i32).max(self.clip[0]);
        let y0 = (floor(y) as i32).max(self.clip[1]);
        let x1 = (ceil(x + w) as i32).min(self.clip[2]);
        let y1 = (ceil(y + h) as i32).min(self.clip[3]);
        let (mx, my) = (x + w / 2.0, y + h / 2.0);
        let stride = self.w as usize;
        let px = self.pixels();
        let sd = |qx: f32, qy: f32, r: f32| {
            if qx > 0.0 && qy > 0.0 { sqrt(qx * qx + qy * qy) - r } else { qx.max(qy) - r }
        };
        for py in y0..y1 {
            let col = pm(mix(top, bot, (py as f32 + 0.5 - y) / h), alpha);
            let ay = abs(py as f32 + 0.5 - my);
            // the middle of the ring is empty, so only the two edges are visited
            let inner = inner_half(ay, w / 2.0 - t, h / 2.0 - t, ri, 0.5);
            let (sa, sb) = if inner >= 0.0 {
                ((ceil(mx - inner - 0.5) as i32).max(x0), (floor(mx + inner - 0.5) as i32 + 1).min(x1))
            } else {
                (x1, x1)
            };
            for pxx in (x0..sa.min(x1)).chain(sb.max(sa).max(x0)..x1) {
                let ax = abs(pxx as f32 + 0.5 - mx);
                let d_out = sd(ax - (w / 2.0 - r), ay - (h / 2.0 - r), r);
                let d_in = sd(ax - (w / 2.0 - t - ri), ay - (h / 2.0 - t - ri), ri);
                let c = clamp(0.5 - d_out, 0.0, 1.0) - clamp(0.5 - d_in, 0.0, 1.0);
                if c > 0.0 {
                    let dst = &mut px[py as usize * stride + pxx as usize];
                    *dst = over(*dst, scale(col, (c * 256.0) as u32));
                }
            }
        }
    }

    /// Composite a premultiplied image, scaled into (dx,dy,dw,dh), with alpha 0..=256.
    pub fn blit(&mut self, src: &[u32], sw: i32, sh: i32, dx: f32, dy: f32, dw: f32, dh: f32, alpha: u32) {
        if alpha == 0 || dw < 0.5 || dh < 0.5 || sw <= 0 || sh <= 0 {
            return;
        }
        let x0 = (floor(dx) as i32).max(self.clip[0]);
        let y0 = (floor(dy) as i32).max(self.clip[1]);
        let x1 = (ceil(dx + dw) as i32).min(self.clip[2]);
        let y1 = (ceil(dy + dh) as i32).min(self.clip[3]);
        let stride = self.w as usize;
        let exact = dw as i32 == sw && dh as i32 == sh && dx == floor(dx) && dy == floor(dy);
        let px = self.pixels();
        if exact {
            for py in y0..y1 {
                let sy = py - dy as i32;
                for pxx in x0..x1 {
                    let sx = pxx - dx as i32;
                    let s = src[(sy * sw + sx) as usize];
                    if s != 0 {
                        let d = &mut px[py as usize * stride + pxx as usize];
                        *d = over(*d, if alpha >= 256 { s } else { scale(s, alpha) });
                    }
                }
            }
            return;
        }
        let kx = sw as f32 / dw;
        let ky = sh as f32 / dh;
        let fetch = |x: i32, y: i32| -> u32 {
            if x < 0 || y < 0 || x >= sw || y >= sh { 0 } else { src[(y * sw + x) as usize] }
        };
        // No box prefilter is needed because we never shrink by more than about 2x.
        for py in y0..y1 {
            let fy = (py as f32 + 0.5 - dy) * ky - 0.5;
            let iy = floor(fy) as i32;
            let ty = ((fy - iy as f32) * 256.0) as u32;
            for pxx in x0..x1 {
                let fx = (pxx as f32 + 0.5 - dx) * kx - 0.5;
                let ix = floor(fx) as i32;
                let tx = ((fx - ix as f32) * 256.0) as u32;
                let a = fetch(ix, iy);
                let b = fetch(ix + 1, iy);
                let c = fetch(ix, iy + 1);
                let e = fetch(ix + 1, iy + 1);
                if (a | b | c | e) == 0 {
                    continue;
                }
                let top = scale(a, 256 - tx) + scale(b, tx);
                let bot = scale(c, 256 - tx) + scale(e, tx);
                let mut s = scale(top, 256 - ty) + scale(bot, ty);
                if alpha < 256 {
                    s = scale(s, alpha);
                }
                let d = &mut px[py as usize * stride + pxx as usize];
                *d = over(*d, s);
            }
        }
    }

    /// Composite an 8-bit coverage mask with a solid premultiplied color.
    pub fn mask(&mut self, m: &[u8], mw: i32, mh: i32, dx: i32, dy: i32, col: u32) {
        let x0 = dx.max(self.clip[0]);
        let y0 = dy.max(self.clip[1]);
        let x1 = (dx + mw).min(self.clip[2]);
        let y1 = (dy + mh).min(self.clip[3]);
        let stride = self.w as usize;
        let px = self.pixels();
        for py in y0..y1 {
            let mrow = ((py - dy) * mw) as usize;
            for pxx in x0..x1 {
                let a = m[mrow + (pxx - dx) as usize] as u32;
                if a != 0 {
                    let d = &mut px[py as usize * stride + pxx as usize];
                    *d = over(*d, scale(col, a + (a >> 7)));
                }
            }
        }
    }

    /// Fill coverage with a per-pixel paint function returning premultiplied color.
    pub fn fill_cov<F: Fn(i32, i32) -> u32>(&mut self, cov: &Coverage, paint: F) {
        let w = (cov.w as i32).min(self.w);
        let h = (cov.h as i32).min(self.h);
        let stride = self.w as usize;
        let px = self.pixels();
        for y in 0..h {
            for x in 0..w {
                let a = cov.a[y as usize * cov.w + x as usize] as u32;
                if a != 0 {
                    let c = paint(x, y);
                    let d = &mut px[y as usize * stride + x as usize];
                    *d = over(*d, scale(c, a + (a >> 7)));
                }
            }
        }
    }

    /// Ensures every pixel has alpha >= 1 inside a rect so it stays hit-testable.
    pub fn hit_fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let stride = self.w as usize;
        let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(self.w), y1.min(self.h));
        let px = self.pixels();
        for y in y0..y1 {
            for v in &mut px[y as usize * stride + x0 as usize..y as usize * stride + x1 as usize] {
                if *v >> 24 == 0 {
                    *v = 0x0100_0000;
                }
            }
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        if self.dc != 0 {
            unsafe {
                SelectObject(self.dc, self.old);
                DeleteObject(self.bmp);
                DeleteDC(self.dc);
            }
        }
    }
}

/// Scales a premultiplied image down to exactly s by s pixels, keeping the aspect ratio and centering it.
pub fn fit_square(src: &[u32], sw: i32, sh: i32, s: i32) -> alloc::vec::Vec<u32> {
    let mut c = Canvas::mem(s, s);
    if sw <= 0 || sh <= 0 {
        return c.into_vec();
    }
    let k = (s as f32 / sw as f32).min(s as f32 / sh as f32);
    let (dw, dh) = (sw as f32 * k, sh as f32 * k);
    if k < 0.5 {
        // area-average to an intermediate of ~2x the target, then bilinear
        let f = (1.0 / (k * 2.0)) as i32;
        let (iw, ih) = (sw / f, sh / f);
        let mut tmp = alloc::vec![0u32; (iw * ih) as usize];
        for y in 0..ih {
            for x in 0..iw {
                let (mut a, mut r, mut g, mut b) = (0u32, 0u32, 0u32, 0u32);
                for yy in 0..f {
                    for xx in 0..f {
                        let v = src[((y * f + yy) * sw + x * f + xx) as usize];
                        a += v >> 24;
                        r += (v >> 16) & 255;
                        g += (v >> 8) & 255;
                        b += v & 255;
                    }
                }
                let n = (f * f) as u32;
                tmp[(y * iw + x) as usize] = ((a / n) << 24) | ((r / n) << 16) | ((g / n) << 8) | (b / n);
            }
        }
        c.blit(&tmp, iw, ih, (s as f32 - dw) / 2.0, (s as f32 - dh) / 2.0, dw, dh, 256);
    } else {
        c.blit(src, sw, sh, (s as f32 - dw) / 2.0, (s as f32 - dh) / 2.0, dw, dh, 256);
    }
    c.into_vec()
}

/// Half width of the span that is fully inside a rounded rect (half sizes hw,
/// hh, radius r) shrunk by `inset`, for a row at vertical distance `dy` from
/// the center. Negative when the row has no such span.
fn inner_half(dy: f32, hw: f32, hh: f32, r: f32, inset: f32) -> f32 {
    let (iw, ih) = (hw - inset, hh - inset);
    if iw <= 0.0 || ih <= 0.0 || dy > ih {
        return -1.0;
    }
    let ri = (r - inset).max(0.0);
    let qy = dy - (ih - ri);
    if qy <= 0.0 {
        return iw;
    }
    if qy >= ri {
        return -1.0;
    }
    iw - ri + sqrt(ri * ri - qy * qy)
}
