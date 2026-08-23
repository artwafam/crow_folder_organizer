//! Small anti-aliased vector rasterizer based on signed area accumulation, like
//! font-rs. Paths are flattened to polylines in device space. Strokes become
//! polygons that all share one orientation, so overlapping parts combine well.

use crate::util::*;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Default, PartialEq)]
pub struct P {
    pub x: f32,
    pub y: f32,
}
#[inline]
pub fn p(x: f32, y: f32) -> P {
    P { x, y }
}

/// 2D affine transform with rows (a c e) and (b d f).
#[derive(Clone, Copy)]
pub struct Mat {
    pub a: f32, pub b: f32, pub c: f32, pub d: f32, pub e: f32, pub f: f32,
}
impl Mat {
    pub const ID: Mat = Mat { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
    pub fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Mat {
        Mat { a, b, c, d, e, f }
    }
    pub fn translate(x: f32, y: f32) -> Mat {
        Mat::new(1.0, 0.0, 0.0, 1.0, x, y)
    }
    pub fn scale(x: f32, y: f32) -> Mat {
        Mat::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }
    pub fn rotate(rad: f32) -> Mat {
        let (s, c) = (sin(rad), cos(rad));
        Mat::new(c, s, -s, c, 0.0, 0.0)
    }
    /// self * o  (apply `o` first, then `self`)
    pub fn mul(&self, o: &Mat) -> Mat {
        Mat {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
    #[inline]
    pub fn apply(&self, q: P) -> P {
        P { x: self.a * q.x + self.c * q.y + self.e, y: self.b * q.x + self.d * q.y + self.f }
    }
    pub fn invert(&self) -> Mat {
        let det = self.a * self.d - self.b * self.c;
        if abs(det) < 1e-12 {
            return Mat::ID;
        }
        let id = 1.0 / det;
        Mat {
            a: self.d * id,
            b: -self.b * id,
            c: -self.c * id,
            d: self.a * id,
            e: (self.c * self.f - self.d * self.e) * id,
            f: (self.b * self.e - self.a * self.f) * id,
        }
    }
    /// Approximate uniform scale factor (for stroke widths / tolerance).
    pub fn scale_factor(&self) -> f32 {
        sqrt(abs(self.a * self.d - self.b * self.c))
    }
}

/// A flattened path, stored as polylines in device space.
pub struct Path {
    pub polys: Vec<Vec<P>>,
    pub closed: Vec<bool>,
    pub m: Mat,
    start: P,
    cur: P, // user-space current point
}

impl Path {
    pub fn new(m: Mat) -> Path {
        Path { polys: Vec::new(), closed: Vec::new(), m, start: P::default(), cur: P::default() }
    }
    pub fn cur(&self) -> P {
        self.cur
    }
    pub fn move_to(&mut self, q: P) {
        self.polys.push(vec![self.m.apply(q)]);
        self.closed.push(false);
        self.start = q;
        self.cur = q;
    }
    fn ensure(&mut self) {
        if self.polys.is_empty() {
            let c = self.cur;
            self.move_to(c);
        }
    }
    pub fn line_to(&mut self, q: P) {
        self.ensure();
        let d = self.m.apply(q);
        self.polys.last_mut().unwrap().push(d);
        self.cur = q;
    }
    pub fn quad_to(&mut self, c: P, q: P) {
        let p0 = self.cur;
        // elevate to cubic
        let c1 = p(p0.x + 2.0 / 3.0 * (c.x - p0.x), p0.y + 2.0 / 3.0 * (c.y - p0.y));
        let c2 = p(q.x + 2.0 / 3.0 * (c.x - q.x), q.y + 2.0 / 3.0 * (c.y - q.y));
        self.cubic_to(c1, c2, q);
    }
    pub fn cubic_to(&mut self, c1: P, c2: P, q: P) {
        self.ensure();
        let m = self.m;
        let (a, b, c, d) = (m.apply(self.cur), m.apply(c1), m.apply(c2), m.apply(q));
        let dd = abs(a.x - 2.0 * b.x + c.x) + abs(a.y - 2.0 * b.y + c.y)
            + abs(b.x - 2.0 * c.x + d.x) + abs(b.y - 2.0 * c.y + d.y);
        let n = clamp(ceil(sqrt(dd * 3.0)), 1.0, 64.0) as i32;
        let poly = self.polys.last_mut().unwrap();
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            poly.push(p(
                w0 * a.x + w1 * b.x + w2 * c.x + w3 * d.x,
                w0 * a.y + w1 * b.y + w2 * c.y + w3 * d.y,
            ));
        }
        self.cur = q;
    }
    /// SVG elliptical arc (endpoint parameterization).
    pub fn arc_to(&mut self, mut rx: f32, mut ry: f32, xrot_deg: f32, large: bool, sweep: bool, q: P) {
        let p0 = self.cur;
        if rx == 0.0 || ry == 0.0 || (p0.x == q.x && p0.y == q.y) {
            self.line_to(q);
            return;
        }
        rx = abs(rx);
        ry = abs(ry);
        let phi = xrot_deg * PI / 180.0;
        let (sp, cp) = (sin(phi), cos(phi));
        let dx = (p0.x - q.x) / 2.0;
        let dy = (p0.y - q.y) / 2.0;
        let x1 = cp * dx + sp * dy;
        let y1 = -sp * dx + cp * dy;
        let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lam > 1.0 {
            let s = sqrt(lam);
            rx *= s;
            ry *= s;
        }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let mut co = if den == 0.0 { 0.0 } else { sqrt((num / den).max(0.0)) };
        if large == sweep {
            co = -co;
        }
        let cxp = co * rx * y1 / ry;
        let cyp = -co * ry * x1 / rx;
        let cx = cp * cxp - sp * cyp + (p0.x + q.x) / 2.0;
        let cy = sp * cxp + cp * cyp + (p0.y + q.y) / 2.0;
        let ang = |ux: f32, uy: f32, vx: f32, vy: f32| atan2(ux * vy - uy * vx, ux * vx + uy * vy);
        let th1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
        let mut dth = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
        if !sweep && dth > 0.0 {
            dth -= 2.0 * PI;
        } else if sweep && dth < 0.0 {
            dth += 2.0 * PI;
        }
        let segs = ceil(abs(dth) / (PI / 2.0)).max(1.0) as i32;
        let delta = dth / segs as f32;
        let t = 4.0 / 3.0 * (sin(delta / 4.0) / cos(delta / 4.0));
        let mut th = th1;
        for i in 0..segs {
            let (c1, s1) = (cos(th), sin(th));
            let th2 = th + delta;
            let (c2, s2) = (cos(th2), sin(th2));
            let ep = |c: f32, s: f32| p(cx + rx * c * cp - ry * s * sp, cy + rx * c * sp + ry * s * cp);
            let dp = |c: f32, s: f32| p(-rx * s * cp - ry * c * sp, -rx * s * sp + ry * c * cp);
            let a = ep(c1, s1);
            let da = dp(c1, s1);
            let b = ep(c2, s2);
            let db = dp(c2, s2);
            let end = if i == segs - 1 { q } else { b };
            self.cubic_to(p(a.x + t * da.x, a.y + t * da.y), p(b.x - t * db.x, b.y - t * db.y), end);
            th = th2;
        }
    }
    pub fn close(&mut self) {
        if let Some(c) = self.closed.last_mut() {
            *c = true;
        }
        self.cur = self.start;
    }
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.move_to(p(x, y));
        self.line_to(p(x + w, y));
        self.line_to(p(x + w, y + h));
        self.line_to(p(x, y + h));
        self.close();
    }
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) {
        let rx = rx.min(w / 2.0);
        let ry = ry.min(h / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            return self.rect(x, y, w, h);
        }
        self.move_to(p(x + rx, y));
        self.line_to(p(x + w - rx, y));
        self.arc_to(rx, ry, 0.0, false, true, p(x + w, y + ry));
        self.line_to(p(x + w, y + h - ry));
        self.arc_to(rx, ry, 0.0, false, true, p(x + w - rx, y + h));
        self.line_to(p(x + rx, y + h));
        self.arc_to(rx, ry, 0.0, false, true, p(x, y + h - ry));
        self.line_to(p(x, y + ry));
        self.arc_to(rx, ry, 0.0, false, true, p(x + rx, y));
        self.close();
    }
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
        self.move_to(p(cx + rx, cy));
        self.arc_to(rx, ry, 0.0, false, true, p(cx - rx, cy));
        self.arc_to(rx, ry, 0.0, false, true, p(cx + rx, cy));
        self.close();
    }

    /// Expands this path's outline into a polygon set representing its stroke.
    pub fn stroke(&self, width_user: f32, round_cap: bool) -> Path {
        let hw = width_user * self.m.scale_factor() * 0.5;
        let mut out = Path::new(Mat::ID);
        if hw <= 0.0 {
            return out;
        }
        for (i, poly) in self.polys.iter().enumerate() {
            let closed = self.closed[i];
            let n = poly.len();
            if n == 0 {
                continue;
            }
            let segs = if closed { n } else { n - 1 };
            for s in 0..segs {
                let a = poly[s];
                let b = poly[(s + 1) % n];
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let len = sqrt(dx * dx + dy * dy);
                if len < 1e-4 {
                    continue;
                }
                let (nx, ny) = (-dy / len * hw, dx / len * hw);
                out.polys.push(vec![p(a.x + nx, a.y + ny), p(b.x + nx, b.y + ny), p(b.x - nx, b.y - ny), p(a.x - nx, a.y - ny)]);
                out.closed.push(true);
            }
            // Joins and round caps are drawn as discs at the vertices
            for (k, &v) in poly.iter().enumerate() {
                let end = !closed && (k == 0 || k == n - 1);
                if end && !round_cap {
                    continue;
                }
                if n > 2 && !end && k > 0 && k < n - 1 {
                    // skip near-collinear interior vertices (from curve flattening)
                    let (a, b) = (poly[k - 1], poly[k + 1]);
                    let cr = (v.x - a.x) * (b.y - v.y) - (v.y - a.y) * (b.x - v.x);
                    let l1 = sqrt((v.x - a.x) * (v.x - a.x) + (v.y - a.y) * (v.y - a.y));
                    let l2 = sqrt((b.x - v.x) * (b.x - v.x) + (b.y - v.y) * (b.y - v.y));
                    if abs(cr) < 0.02 * l1 * l2 && l1 * l2 > 0.0 {
                        let dot = (v.x - a.x) * (b.x - v.x) + (v.y - a.y) * (b.y - v.y);
                        if dot > 0.0 && hw < 3.0 { /* tiny gap only; still draw for wide strokes */ continue; }
                    }
                }
                out.disc(v, hw);
            }
        }
        out
    }
    /// Disc with orientation matching stroke quads (negative area).
    fn disc(&mut self, c: P, r: f32) {
        let n = clamp(ceil(r * 1.6) + 6.0, 8.0, 40.0) as i32;
        let mut v = Vec::with_capacity(n as usize);
        for i in 0..n {
            let a = -(i as f32) * 2.0 * PI / n as f32;
            v.push(p(c.x + r * cos(a), c.y + r * sin(a)));
        }
        self.polys.push(v);
        self.closed.push(true);
    }

    pub fn bounds(&self) -> (P, P) {
        let mut lo = p(f32::MAX, f32::MAX);
        let mut hi = p(f32::MIN, f32::MIN);
        for poly in &self.polys {
            for q in poly {
                lo.x = lo.x.min(q.x);
                lo.y = lo.y.min(q.y);
                hi.x = hi.x.max(q.x);
                hi.y = hi.y.max(q.y);
            }
        }
        (lo, hi)
    }
}

/// Coverage mask (0..=255) for a w*h area.
pub struct Coverage {
    pub w: usize,
    pub h: usize,
    pub a: Vec<u8>,
}

pub fn rasterize(path: &Path, w: usize, h: usize) -> Coverage {
    let mut acc = vec![0f32; w * h + w + 4];
    for poly in &path.polys {
        let n = poly.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            // all subpaths are implicitly closed for filling
            line(&mut acc, w, h, poly[i], poly[(i + 1) % n]);
        }
    }
    let mut out = vec![0u8; w * h];
    let mut s = 0f32;
    for i in 0..w * h {
        s += acc[i];
        let v = abs(s).min(1.0);
        out[i] = (v * 255.0 + 0.5) as u8;
    }
    Coverage { w, h, a: out }
}

fn line(acc: &mut [f32], w: usize, h: usize, a: P, b: P) {
    if a.y == b.y {
        return;
    }
    let (dir, p0, p1) = if a.y < b.y { (1.0, a, b) } else { (-1.0, b, a) };
    let hf = h as f32;
    if p1.y <= 0.0 || p0.y >= hf {
        return;
    }
    let wf = w as f32;
    let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
    let mut x = p0.x;
    let ystart = if p0.y < 0.0 {
        x -= p0.y * dxdy;
        0
    } else {
        p0.y as usize
    };
    let yend = (ceil(p1.y) as usize).min(h);
    for y in ystart..yend {
        let ls = y * w;
        let yf = y as f32;
        let dy = (yf + 1.0).min(p1.y) - yf.max(p0.y);
        let xnext = x + dxdy * dy;
        let d = dy * dir;
        let (mut x0, mut x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
        x0 = clamp(x0, 0.0, wf);
        x1 = clamp(x1, 0.0, wf);
        let x0f = floor(x0);
        let x0i = x0f as usize;
        let x1c = ceil(x1);
        let x1i = x1c as usize;
        if x1i <= x0i + 1 {
            let xmf = 0.5 * (x0 + x1) - x0f;
            acc[ls + x0i] += d - d * xmf;
            acc[ls + x0i + 1] += d * xmf;
        } else {
            let s = 1.0 / (x1 - x0);
            let x0r = x0 - x0f;
            let a0 = 0.5 * s * (1.0 - x0r) * (1.0 - x0r);
            let x1r = x1 - x1c + 1.0;
            let am = 0.5 * s * x1r * x1r;
            acc[ls + x0i] += d * a0;
            if x1i == x0i + 2 {
                acc[ls + x0i + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - x0r);
                acc[ls + x0i + 1] += d * (a1 - a0);
                for xi in x0i + 2..x1i - 1 {
                    acc[ls + xi] += d * s;
                }
                let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                acc[ls + x1i - 1] += d * (1.0 - a2 - am);
            }
            acc[ls + x1i] += d * am;
        }
        x = xnext;
    }
}
