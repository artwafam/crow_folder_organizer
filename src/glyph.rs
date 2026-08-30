//! Procedural vector folder icon (recolorable, with an animated opening lid).

use crate::gfx::{mix, pm, shade, Canvas};
use crate::raster::*;
use crate::util::*;
use alloc::vec::Vec;

fn fill_grad(cv: &mut Canvas, ph: &Path, top: u32, bot: u32, y0: f32, y1: f32, alpha: u32) {
    let cov = rasterize(ph, cv.w as usize, cv.h as usize);
    let span = (y1 - y0).max(1.0);
    cv.fill_cov(&cov, |_, y| pm(mix(top, bot, (y as f32 - y0) / span), alpha));
}

/// Polygon with rounded corners (radius r in path units).
fn rounded_poly(ph: &mut Path, pts: &[P], r: f32) {
    let n = pts.len();
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        let d1 = (cur.x - prev.x, cur.y - prev.y);
        let d2 = (next.x - cur.x, next.y - cur.y);
        let l1 = sqrt(d1.0 * d1.0 + d1.1 * d1.1).max(1e-6);
        let l2 = sqrt(d2.0 * d2.0 + d2.1 * d2.1).max(1e-6);
        let rr = r.min(l1 / 2.0).min(l2 / 2.0);
        let a = p(cur.x - d1.0 / l1 * rr, cur.y - d1.1 / l1 * rr);
        let b = p(cur.x + d2.0 / l2 * rr, cur.y + d2.1 / l2 * rr);
        if i == 0 {
            ph.move_to(a);
        } else {
            ph.line_to(a);
        }
        ph.quad_to(cur, b);
    }
    ph.close();
}

/// Draws the folder glyph at s by s pixels. `open` goes from 0 to 1 and animates the front flap.
pub fn folder(s: i32, base: u32, open: f32) -> Vec<u32> {
    let mut cv = Canvas::mem(s, s);
    let k = s as f32;
    let m = Mat::scale(k, k);
    let o = clamp(open, 0.0, 1.0);

    // contact shadow
    cv.rrect(0.12 * k, 0.83 * k, 0.76 * k, 0.07 * k, 0.04 * k, 0x5A000000, 0x5A000000, 255, (0.05 * k).max(1.5));

    // back plate with tab
    let mut back = Path::new(m);
    rounded_poly(&mut back, &[
        p(0.06, 0.13), p(0.38, 0.13), p(0.46, 0.21), p(0.94, 0.21), p(0.94, 0.86), p(0.06, 0.86),
    ], 0.06);
    fill_grad(&mut cv, &back, shade(base, -0.18), shade(base, -0.40), 0.13 * k, 0.86 * k, 255);

    // papers peeking out (rise as the folder opens)
    let lift = 0.05 + o * 0.10;
    let mut paper = Path::new(m);
    rounded_poly(&mut paper, &[p(0.15, 0.30 - lift), p(0.85, 0.30 - lift), p(0.85, 0.80), p(0.15, 0.80)], 0.03);
    fill_grad(&mut cv, &paper, 0xFFF4F4F6, 0xFFDCDDE2, (0.30 - lift) * k, 0.8 * k, 255);
    if o > 0.01 {
        let l2 = lift * 0.55;
        let mut p2 = Path::new(m.mul(&Mat::translate(0.5, 0.6)).mul(&Mat::rotate(-0.06 * o)).mul(&Mat::translate(-0.5, -0.6)));
        rounded_poly(&mut p2, &[p(0.20, 0.34 - l2), p(0.82, 0.34 - l2), p(0.82, 0.80), p(0.20, 0.80)], 0.03);
        fill_grad(&mut cv, &p2, 0xFFFFFFFF, 0xFFE9EAEE, (0.34 - l2) * k, 0.8 * k, 255);
    }

    // Front flap, which tilts toward the viewer when opening
    let top = 0.33 + o * 0.17;
    let spread = o * 0.035;
    let mut front = Path::new(m);
    rounded_poly(&mut front, &[
        p(0.06 - spread, top), p(0.94 + spread, top), p(0.94, 0.86), p(0.06, 0.86),
    ], 0.06);
    fill_grad(&mut cv, &front, shade(base, 0.10 - o * 0.06), shade(base, -0.10), top * k, 0.86 * k, 255);

    // glossy rim highlight along the flap's top edge
    let mut rim = Path::new(m);
    rim.move_to(p(0.10 - spread, top + 0.012));
    rim.line_to(p(0.90 + spread, top + 0.012));
    let st = rim.stroke((1.2 / k).max(0.012), true);
    fill_grad(&mut cv, &st, 0x70FFFFFF, 0x70FFFFFF, 0.0, k, 255);

    cv.into_vec()
}

/// Neutral placeholder for items whose icon couldn't be resolved.
pub fn fallback(s: i32) -> Vec<u32> {
    let mut cv = Canvas::mem(s, s);
    let k = s as f32;
    cv.rrect(0.14 * k, 0.10 * k, 0.72 * k, 0.80 * k, 0.12 * k, 0xFFE8EAF0, 0xFFB9BECB, 255, 1.0);
    let mut ph = Path::new(Mat::scale(k, k));
    ph.move_to(p(0.32, 0.42));
    ph.line_to(p(0.68, 0.42));
    ph.move_to(p(0.32, 0.56));
    ph.line_to(p(0.60, 0.56));
    let st = ph.stroke(0.05, true);
    fill_grad(&mut cv, &st, 0xFF7A8090, 0xFF7A8090, 0.0, k, 255);
    cv.into_vec()
}

/// Default panel glyph, s by s pixels: a card with a header strip and a small
/// grid of tiles, colored like the panel. `rolled` shows only the header.
pub fn panel(s: i32, base: u32, rolled: f32) -> Vec<u32> {
    let mut cv = Canvas::mem(s, s);
    let k = s as f32;
    let r = clamp(rolled, 0.0, 1.0);
    let top = 0.16;
    let bottom = lerp(0.84, 0.40, r);
    cv.rrect(0.12 * k, (bottom - 0.02) * k, 0.76 * k, 0.07 * k, 0.04 * k, 0x50000000, 0x50000000, 255, (0.05 * k).max(1.5));
    // card body
    cv.rrect(0.08 * k, top * k, 0.84 * k, (bottom - top) * k, 0.12 * k, shade(base, 0.06), shade(base, -0.18), 255, 1.0);
    // header strip
    let hb = (0.30 * k).min((bottom - top) * k);
    let mut ph = Path::new(Mat::ID);
    ph.rrect(0.08 * k, top * k, 0.84 * k, hb, 0.12 * k, 0.12 * k);
    let cov = rasterize(&ph, s as usize, s as usize);
    let hc = pm(shade(base, 0.30), 255);
    cv.fill_cov(&cov, |_, y| if (y as f32) < top * k + hb - 0.10 * k { hc } else { 0 });
    // tiles fade out as the card rolls up
    let ta = ((1.0 - r * 2.0).max(0.0) * 235.0) as u32;
    if ta > 0 {
        for row in 0..2 {
            for col in 0..3 {
                let x = (0.19 + col as f32 * 0.22) * k;
                let y = (0.52 + row as f32 * 0.17) * k;
                cv.rrect(x, y, 0.16 * k, 0.12 * k, 0.03 * k, 0xFFFFFFFF, 0xFFE6E8EE, ta, 1.0);
            }
        }
    }
    cv.into_vec()
}
