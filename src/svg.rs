//! Compact SVG renderer that covers what icon sets use. It supports path, rect,
//! circle, ellipse, line, polyline, polygon, groups, transforms, fill, stroke,
//! opacity, style attributes, currentColor and linear and radial gradients.

use crate::gfx::{mix, pm, Canvas};
use crate::raster::*;
use crate::util::*;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq)]
enum Paint {
    None,
    Color(u32),
    Grad(usize),
    Current,
}

#[derive(Clone, Copy)]
struct Style {
    fill: Paint,
    stroke: Paint,
    stroke_w: f32,
    opacity: f32,
    fill_op: f32,
    stroke_op: f32,
    round_cap: bool,
    m: Mat,
    hidden: bool,
}

struct Grad {
    id: Vec<u8>,
    radial: bool,
    user_space: bool,
    v: [f32; 5], // x1 y1 x2 y2 | cx cy r fx fy (radial uses cx,cy,r)
    stops: Vec<(f32, u32)>,
    href: Vec<u8>,
}

struct Tag<'a> {
    name: &'a [u8],
    attrs: Vec<(&'a [u8], &'a [u8])>,
    close: bool,
    selfclose: bool,
}

fn tags(src: &[u8]) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = src.len();
    while i < n {
        if src[i] != b'<' {
            i += 1;
            continue;
        }
        if src[i..].starts_with(b"<!--") {
            match find(&src[i..], b"-->") {
                Some(e) => i += e + 3,
                None => break,
            }
            continue;
        }
        if src[i..].starts_with(b"<?") || src[i..].starts_with(b"<!") {
            match find(&src[i..], b">") {
                Some(e) => i += e + 1,
                None => break,
            }
            continue;
        }
        i += 1;
        let close = i < n && src[i] == b'/';
        if close {
            i += 1;
        }
        let ns = i;
        while i < n && !src[i].is_ascii_whitespace() && src[i] != b'>' && src[i] != b'/' {
            i += 1;
        }
        let name = strip_ns(&src[ns..i]);
        let mut attrs = Vec::new();
        let mut selfclose = false;
        loop {
            while i < n && src[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= n {
                break;
            }
            if src[i] == b'>' {
                i += 1;
                break;
            }
            if src[i] == b'/' {
                selfclose = true;
                i += 1;
                continue;
            }
            let ks = i;
            while i < n && src[i] != b'=' && !src[i].is_ascii_whitespace() && src[i] != b'>' && src[i] != b'/' {
                i += 1;
            }
            let k = &src[ks..i];
            while i < n && src[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < n && src[i] == b'=' {
                i += 1;
                while i < n && src[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < n && (src[i] == b'"' || src[i] == b'\'') {
                    let q = src[i];
                    i += 1;
                    let vs = i;
                    while i < n && src[i] != q {
                        i += 1;
                    }
                    attrs.push((strip_ns(k), &src[vs..i]));
                    i += 1;
                }
            }
        }
        out.push(Tag { name, attrs, close, selfclose });
    }
    out
}

fn strip_ns(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == b':') {
        Some(p) if !s.starts_with(b"xml") || p != 3 => &s[p + 1..],
        _ => s,
    }
}
fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

// ------------------------------------------------------------ number lexing
struct Lex<'a> {
    s: &'a [u8],
    i: usize,
}
impl<'a> Lex<'a> {
    fn new(s: &'a [u8]) -> Self {
        Lex { s, i: 0 }
    }
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }
    fn num(&mut self) -> Option<f32> {
        self.skip();
        let s = self.s;
        let mut i = self.i;
        let start = i;
        if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
            i += 1;
        }
        let mut v: f64 = 0.0;
        let mut digits = false;
        while i < s.len() && s[i].is_ascii_digit() {
            v = v * 10.0 + (s[i] - b'0') as f64;
            i += 1;
            digits = true;
        }
        if i < s.len() && s[i] == b'.' {
            i += 1;
            let mut f = 0.1;
            while i < s.len() && s[i].is_ascii_digit() {
                v += (s[i] - b'0') as f64 * f;
                f *= 0.1;
                i += 1;
                digits = true;
            }
        }
        if !digits {
            return None;
        }
        if i < s.len() && (s[i] == b'e' || s[i] == b'E') && i + 1 < s.len() && (s[i + 1].is_ascii_digit() || s[i + 1] == b'-' || s[i + 1] == b'+') {
            i += 1;
            let neg = s[i] == b'-';
            if s[i] == b'-' || s[i] == b'+' {
                i += 1;
            }
            let mut e = 0i32;
            while i < s.len() && s[i].is_ascii_digit() {
                e = e * 10 + (s[i] - b'0') as i32;
                i += 1;
            }
            let mut k = 1.0f64;
            for _ in 0..e.min(38) {
                k *= 10.0;
            }
            if neg { v /= k } else { v *= k }
        }
        if s[start] == b'-' {
            v = -v;
        }
        self.i = i;
        Some(v as f32)
    }
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.s.get(self.i)?;
        self.i += 1;
        Some(c == b'1')
    }
}

fn num(s: &[u8]) -> f32 {
    Lex::new(s).num().unwrap_or(0.0)
}
fn num_pct(s: &[u8]) -> f32 {
    let v = num(s);
    if s.ends_with(b"%") { v / 100.0 } else { v }
}

// ----------------------------------------------------------------- colors
pub fn parse_color(s: &[u8]) -> Option<u32> {
    let s = trim(s);
    if s.first() == Some(&b'#') {
        let h = &s[1..];
        let hv: Vec<u32> = h.iter().filter_map(|&c| hex_val(c)).collect();
        if hv.len() != h.len() {
            return None;
        }
        return match hv.len() {
            3 => Some(0xFF00_0000 | (hv[0] * 17) << 16 | (hv[1] * 17) << 8 | hv[2] * 17),
            6 => Some(0xFF00_0000 | hv.iter().fold(0, |a, &d| a << 4 | d)),
            8 => {
                let v = hv.iter().fold(0u32, |a, &d| a << 4 | d);
                Some((v >> 8) | (v << 24)) // RRGGBBAA -> AARRGGBB
            }
            _ => None,
        };
    }
    if s.starts_with(b"rgb") {
        let open = s.iter().position(|&c| c == b'(')?;
        let mut lx = Lex::new(&s[open + 1..]);
        let mut c = [0f32; 4];
        c[3] = 1.0;
        for (k, slot) in c.iter_mut().enumerate() {
            match lx.num() {
                Some(v) => {
                    let pct = lx.s.get(lx.i) == Some(&b'%');
                    if pct {
                        lx.i += 1;
                    }
                    *slot = if pct { v * if k < 3 { 2.55 } else { 0.01 } } else { v };
                }
                None => break,
            }
            if lx.peek() == Some(b'/') {
                lx.i += 1;
            }
        }
        let cl = |v: f32| clamp(v, 0.0, 255.0) as u32;
        return Some(cl(c[3] * 255.0) << 24 | cl(c[0]) << 16 | cl(c[1]) << 8 | cl(c[2]));
    }
    const NAMES: &[(&[u8], u32)] = &[
        (b"black", 0x000000), (b"white", 0xFFFFFF), (b"red", 0xFF0000), (b"green", 0x008000),
        (b"lime", 0x00FF00), (b"blue", 0x0000FF), (b"yellow", 0xFFFF00), (b"orange", 0xFFA500),
        (b"purple", 0x800080), (b"gray", 0x808080), (b"grey", 0x808080), (b"silver", 0xC0C0C0),
        (b"navy", 0x000080), (b"teal", 0x008080), (b"maroon", 0x800000), (b"olive", 0x808000),
        (b"cyan", 0x00FFFF), (b"aqua", 0x00FFFF), (b"magenta", 0xFF00FF), (b"fuchsia", 0xFF00FF),
        (b"pink", 0xFFC0CB), (b"gold", 0xFFD700), (b"brown", 0xA52A2A), (b"darkgray", 0xA9A9A9),
        (b"lightgray", 0xD3D3D3), (b"dimgray", 0x696969), (b"whitesmoke", 0xF5F5F5),
    ];
    for &(n, v) in NAMES {
        if s.eq_ignore_ascii_case(n) {
            return Some(0xFF00_0000 | v);
        }
    }
    if s.eq_ignore_ascii_case(b"transparent") {
        return Some(0);
    }
    None
}

fn trim(s: &[u8]) -> &[u8] {
    let a = s.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(s.len());
    let b = s.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(a, |p| p + 1);
    &s[a..b]
}

fn parse_transform(s: &[u8]) -> Mat {
    let mut m = Mat::ID;
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && !s[i].is_ascii_alphabetic() {
            i += 1;
        }
        let ns = i;
        while i < s.len() && s[i].is_ascii_alphabetic() {
            i += 1;
        }
        let name = &s[ns..i];
        let open = match s[i..].iter().position(|&c| c == b'(') {
            Some(p) => i + p + 1,
            None => break,
        };
        let close = match s[open..].iter().position(|&c| c == b')') {
            Some(p) => open + p,
            None => break,
        };
        let mut lx = Lex::new(&s[open..close]);
        let mut a = [0f32; 6];
        let mut k = 0;
        while k < 6 {
            match lx.num() {
                Some(v) => {
                    a[k] = v;
                    k += 1;
                }
                None => break,
            }
        }
        let t = match name {
            b"matrix" => Mat::new(a[0], a[1], a[2], a[3], a[4], a[5]),
            b"translate" => Mat::translate(a[0], a[1]),
            b"scale" => Mat::scale(a[0], if k > 1 { a[1] } else { a[0] }),
            b"rotate" => {
                let r = Mat::rotate(a[0] * PI / 180.0);
                if k >= 3 {
                    Mat::translate(a[1], a[2]).mul(&r).mul(&Mat::translate(-a[1], -a[2]))
                } else {
                    r
                }
            }
            b"skewX" => Mat::new(1.0, 0.0, sin(a[0] * PI / 180.0) / cos(a[0] * PI / 180.0), 1.0, 0.0, 0.0),
            b"skewY" => Mat::new(1.0, sin(a[0] * PI / 180.0) / cos(a[0] * PI / 180.0), 0.0, 1.0, 0.0, 0.0),
            _ => Mat::ID,
        };
        m = m.mul(&t);
        i = close + 1;
    }
    m
}

fn parse_paint(v: &[u8], grads: &[Grad]) -> Paint {
    let v = trim(v);
    if v == b"none" {
        return Paint::None;
    }
    if v.eq_ignore_ascii_case(b"currentColor") {
        return Paint::Current;
    }
    if v.starts_with(b"url(") {
        let inner = trim(&v[4..v.iter().position(|&c| c == b')').unwrap_or(v.len())]);
        let id = inner.strip_prefix(b"#").unwrap_or(inner);
        let id = trim(id);
        let id = id.strip_prefix(b"'").and_then(|x| x.strip_suffix(b"'")).unwrap_or(id);
        if let Some(i) = grads.iter().position(|g| g.id == id) {
            return Paint::Grad(i);
        }
        return Paint::Color(0xFF00_0000);
    }
    match parse_color(v) {
        Some(c) => Paint::Color(c),
        None => Paint::None,
    }
}

/// Applies an attribute (or style property) to a style.
fn apply(st: &mut Style, k: &[u8], v: &[u8], grads: &[Grad]) {
    match k {
        b"fill" => st.fill = parse_paint(v, grads),
        b"stroke" => st.stroke = parse_paint(v, grads),
        b"stroke-width" => st.stroke_w = num(v),
        b"opacity" => st.opacity *= num_pct(v),
        b"fill-opacity" => st.fill_op = num_pct(v),
        b"stroke-opacity" => st.stroke_op = num_pct(v),
        b"stroke-linecap" => st.round_cap = trim(v) == b"round" || trim(v) == b"square",
        b"display" => st.hidden |= trim(v) == b"none",
        b"visibility" => st.hidden |= trim(v) == b"hidden",
        b"style" => {
            for decl in v.split(|&c| c == b';') {
                if let Some(c) = decl.iter().position(|&c| c == b':') {
                    let (kk, vv) = (trim(&decl[..c]), trim(&decl[c + 1..]));
                    if kk != b"style" && kk != b"transform" {
                        apply(st, kk, vv, grads);
                    }
                }
            }
        }
        _ => {}
    }
}

fn attr<'a>(t: &Tag<'a>, k: &[u8]) -> Option<&'a [u8]> {
    t.attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| *v)
}
fn attr_f(t: &Tag, k: &[u8], d: f32) -> f32 {
    attr(t, k).map(num).unwrap_or(d)
}

fn collect_grads(tags: &[Tag]) -> Vec<Grad> {
    let mut out: Vec<Grad> = Vec::new();
    let mut cur: Option<usize> = None;
    for t in tags {
        let lin = t.name == b"linearGradient";
        let rad = t.name == b"radialGradient";
        if (lin || rad) && !t.close {
            let v = if lin {
                [attr(t, b"x1").map_or(0.0, num_pct), attr(t, b"y1").map_or(0.0, num_pct),
                 attr(t, b"x2").map_or(1.0, num_pct), attr(t, b"y2").map_or(0.0, num_pct), 0.0]
            } else {
                [attr(t, b"cx").map_or(0.5, num_pct), attr(t, b"cy").map_or(0.5, num_pct),
                 attr(t, b"r").map_or(0.5, num_pct), 0.0, 0.0]
            };
            out.push(Grad {
                id: attr(t, b"id").unwrap_or(b"").to_vec(),
                radial: rad,
                user_space: attr(t, b"gradientUnits") == Some(b"userSpaceOnUse"),
                v,
                stops: Vec::new(),
                href: attr(t, b"href").map(|h| h.strip_prefix(b"#").unwrap_or(h).to_vec()).unwrap_or_default(),
            });
            cur = if t.selfclose { None } else { Some(out.len() - 1) };
        } else if (lin || rad) && t.close {
            cur = None;
        } else if t.name == b"stop" && !t.close {
            if let Some(g) = cur {
                let mut color = 0xFF00_0000u32;
                let mut op = 1.0;
                let mut off = attr(t, b"offset").map_or(0.0, num_pct);
                let mut take = |k: &[u8], v: &[u8]| match k {
                    b"stop-color" => color = parse_color(v).unwrap_or(color),
                    b"stop-opacity" => op = num_pct(v),
                    b"offset" => off = num_pct(v),
                    _ => {}
                };
                for (k, v) in &t.attrs {
                    if *k == b"style" {
                        for decl in v.split(|&c| c == b';') {
                            if let Some(c) = decl.iter().position(|&c| c == b':') {
                                take(trim(&decl[..c]), trim(&decl[c + 1..]));
                            }
                        }
                    } else {
                        take(k, v);
                    }
                }
                let a = (((color >> 24) as f32) * clamp(op, 0.0, 1.0)) as u32;
                out[g].stops.push((clamp(off, 0.0, 1.0), (color & 0xFF_FFFF) | a << 24));
            }
        }
    }
    // resolve href'd stops
    for i in 0..out.len() {
        if out[i].stops.is_empty() && !out[i].href.is_empty() {
            if let Some(j) = out.iter().position(|g| g.id == out[i].href) {
                out[i].stops = out[j].stops.clone();
            }
        }
    }
    out
}

fn grad_color(g: &Grad, t: f32) -> u32 {
    let s = &g.stops;
    if s.is_empty() {
        return 0;
    }
    if t <= s[0].0 {
        return s[0].1;
    }
    for w in s.windows(2) {
        if t <= w[1].0 {
            let span = w[1].0 - w[0].0;
            return if span <= 0.0 { w[1].1 } else { mix(w[0].1, w[1].1, (t - w[0].0) / span) };
        }
    }
    s[s.len() - 1].1
}

fn draw(cv: &mut Canvas, path: &Path, cov_path: &Path, paint: Paint, op: f32, grads: &[Grad], cur: u32, bbox_user: (P, P)) {
    let a = (clamp(op, 0.0, 1.0) * 255.0) as u32;
    if a == 0 {
        return;
    }
    let cov = rasterize(cov_path, cv.w as usize, cv.h as usize);
    match paint {
        Paint::None => {}
        Paint::Color(c) => {
            let c = pm(c, a);
            cv.fill_cov(&cov, |_, _| c);
        }
        Paint::Current => {
            let c = pm(cur, a);
            cv.fill_cov(&cov, |_, _| c);
        }
        Paint::Grad(i) => {
            let g = &grads[i];
            // gradient space -> user space
            let (lo, hi) = bbox_user;
            let bb = if g.user_space { Mat::ID } else { Mat::new(hi.x - lo.x, 0.0, 0.0, hi.y - lo.y, lo.x, lo.y) };
            let inv = path.m.mul(&bb).invert();
            let v = g.v;
            cv.fill_cov(&cov, |x, y| {
                let q = inv.apply(p(x as f32 + 0.5, y as f32 + 0.5));
                let t = if g.radial {
                    sqrt((q.x - v[0]) * (q.x - v[0]) + (q.y - v[1]) * (q.y - v[1])) / v[2].max(1e-6)
                } else {
                    let (dx, dy) = (v[2] - v[0], v[3] - v[1]);
                    let l2 = (dx * dx + dy * dy).max(1e-9);
                    ((q.x - v[0]) * dx + (q.y - v[1]) * dy) / l2
                };
                pm(grad_color(g, clamp(t, 0.0, 1.0)), a)
            });
        }
    }
}

fn build_path(t: &Tag, m: Mat) -> Option<(Path, bool)> {
    let mut ph = Path::new(m);
    let mut closed_shape = true;
    match t.name {
        b"path" => {
            parse_d(&mut ph, attr(t, b"d")?);
            closed_shape = false;
        }
        b"rect" => {
            let (w, h) = (attr_f(t, b"width", 0.0), attr_f(t, b"height", 0.0));
            let mut rx = attr(t, b"rx").map(num);
            let mut ry = attr(t, b"ry").map(num);
            if rx.is_none() {
                rx = ry;
            }
            if ry.is_none() {
                ry = rx;
            }
            ph.rrect(attr_f(t, b"x", 0.0), attr_f(t, b"y", 0.0), w, h, rx.unwrap_or(0.0), ry.unwrap_or(0.0));
        }
        b"circle" => {
            let r = attr_f(t, b"r", 0.0);
            ph.ellipse(attr_f(t, b"cx", 0.0), attr_f(t, b"cy", 0.0), r, r);
        }
        b"ellipse" => {
            ph.ellipse(attr_f(t, b"cx", 0.0), attr_f(t, b"cy", 0.0), attr_f(t, b"rx", 0.0), attr_f(t, b"ry", 0.0));
        }
        b"line" => {
            ph.move_to(p(attr_f(t, b"x1", 0.0), attr_f(t, b"y1", 0.0)));
            ph.line_to(p(attr_f(t, b"x2", 0.0), attr_f(t, b"y2", 0.0)));
            closed_shape = false;
        }
        b"polyline" | b"polygon" => {
            let mut lx = Lex::new(attr(t, b"points")?);
            let mut first = true;
            while let (Some(x), Some(y)) = (lx.num(), lx.num()) {
                if first { ph.move_to(p(x, y)) } else { ph.line_to(p(x, y)) }
                first = false;
            }
            if t.name == b"polygon" {
                ph.close();
            } else {
                closed_shape = false;
            }
        }
        _ => return None,
    }
    Some((ph, closed_shape))
}

fn parse_d(ph: &mut Path, d: &[u8]) {
    let mut lx = Lex::new(d);
    let mut cmd = b'M';
    let mut last_ctrl: Option<P> = None;
    let mut last_cmd = b'M';
    loop {
        let c = match lx.peek() {
            None => break,
            Some(c) => c,
        };
        if c.is_ascii_alphabetic() {
            cmd = c;
            lx.i += 1;
            if cmd == b'z' || cmd == b'Z' {
                ph.close();
                last_cmd = cmd;
                last_ctrl = None;
                continue;
            }
        } else if cmd == b'z' || cmd == b'Z' {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let cur = ph.cur();
        let o = if rel { cur } else { P::default() };
        let rd = |lx: &mut Lex| -> Option<P> { Some(p(lx.num()? + o.x, lx.num()? + o.y)) };
        let up = cmd.to_ascii_uppercase();
        let ok = match up {
            b'M' => rd(&mut lx).map(|q| {
                ph.move_to(q);
                cmd = if rel { b'l' } else { b'L' };
            }),
            b'L' => rd(&mut lx).map(|q| ph.line_to(q)),
            b'H' => lx.num().map(|x| ph.line_to(p(x + o.x, cur.y))),
            b'V' => lx.num().map(|y| ph.line_to(p(cur.x, y + o.y))),
            b'C' => (|| {
                let (a, b, q) = (rd(&mut lx)?, rd(&mut lx)?, rd(&mut lx)?);
                ph.cubic_to(a, b, q);
                last_ctrl = Some(b);
                Some(())
            })(),
            b'S' => (|| {
                let (b, q) = (rd(&mut lx)?, rd(&mut lx)?);
                let a = match (last_ctrl, last_cmd.to_ascii_uppercase()) {
                    (Some(lc), b'C' | b'S') => p(2.0 * cur.x - lc.x, 2.0 * cur.y - lc.y),
                    _ => cur,
                };
                ph.cubic_to(a, b, q);
                last_ctrl = Some(b);
                Some(())
            })(),
            b'Q' => (|| {
                let (a, q) = (rd(&mut lx)?, rd(&mut lx)?);
                ph.quad_to(a, q);
                last_ctrl = Some(a);
                Some(())
            })(),
            b'T' => (|| {
                let q = rd(&mut lx)?;
                let a = match (last_ctrl, last_cmd.to_ascii_uppercase()) {
                    (Some(lc), b'Q' | b'T') => p(2.0 * cur.x - lc.x, 2.0 * cur.y - lc.y),
                    _ => cur,
                };
                ph.quad_to(a, q);
                last_ctrl = Some(a);
                Some(())
            })(),
            b'A' => (|| {
                let (rx, ry, rot) = (lx.num()?, lx.num()?, lx.num()?);
                let (la, sw) = (lx.flag()?, lx.flag()?);
                let q = rd(&mut lx)?;
                ph.arc_to(rx, ry, rot, la, sw, q);
                Some(())
            })(),
            _ => None,
        };
        if ok.is_none() {
            break;
        }
        if !matches!(up, b'C' | b'S' | b'Q' | b'T') {
            last_ctrl = None;
        }
        last_cmd = cmd;
    }
}

/// Renders an SVG document into a premultiplied square image of the given size.
pub fn render(src: &[u8], size: i32, current: u32) -> Option<Vec<u32>> {
    let tags = tags(src);
    let grads = collect_grads(&tags);
    let root = tags.iter().find(|t| t.name == b"svg" && !t.close)?;
    let (vx, vy, vw, vh) = match attr(root, b"viewBox") {
        Some(vb) => {
            let mut lx = Lex::new(vb);
            (lx.num()?, lx.num()?, lx.num()?, lx.num()?)
        }
        None => (0.0, 0.0, attr_f(root, b"width", 24.0), attr_f(root, b"height", 24.0)),
    };
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }
    let s = size as f32;
    let k = (s / vw).min(s / vh);
    let base = Mat::translate((s - vw * k) / 2.0, (s - vh * k) / 2.0).mul(&Mat::scale(k, k)).mul(&Mat::translate(-vx, -vy));

    let mut cv = Canvas::mem(size, size);
    let init = Style {
        fill: Paint::Color(0xFF00_0000),
        stroke: Paint::None,
        stroke_w: 1.0,
        opacity: 1.0,
        fill_op: 1.0,
        stroke_op: 1.0,
        round_cap: false,
        m: base,
        hidden: false,
    };
    let mut stack: Vec<Style> = Vec::new();
    let mut st = init;
    let mut skip_depth = 0usize; // inside defs/clipPath/mask/symbol...
    for t in &tags {
        let container = matches!(t.name, b"svg" | b"g" | b"a" | b"switch");
        let skipper = matches!(t.name, b"defs" | b"clipPath" | b"mask" | b"symbol" | b"pattern" | b"marker"
            | b"linearGradient" | b"radialGradient" | b"style" | b"title" | b"desc" | b"metadata" | b"filter");
        if t.close {
            if skipper {
                skip_depth = skip_depth.saturating_sub(1);
            } else if container && skip_depth == 0 {
                st = stack.pop().unwrap_or(init);
            }
            continue;
        }
        if skipper {
            if !t.selfclose {
                skip_depth += 1;
            }
            continue;
        }
        if skip_depth > 0 {
            continue;
        }
        // compute this element's style
        let mut es = st;
        if let Some(tr) = attr(t, b"transform") {
            es.m = es.m.mul(&parse_transform(tr));
        }
        for (k, v) in &t.attrs {
            if *k != b"style" {
                apply(&mut es, k, v, &grads);
            }
        }
        if let Some(v) = attr(t, b"style") {
            apply(&mut es, b"style", v, &grads);
        }
        if container {
            if !t.selfclose {
                stack.push(st);
                st = es;
            }
            continue;
        }
        if es.hidden {
            continue;
        }
        if let Some((ph, closed_shape)) = build_path(t, es.m) {
            let mut bb = Path::new(Mat::ID);
            if let Some((ub, _)) = build_path(t, Mat::ID) {
                bb = ub;
            }
            let bbox = bb.bounds();
            if es.fill != Paint::None && (closed_shape || t.name == b"path" || t.name == b"polyline") {
                draw(&mut cv, &ph, &ph, es.fill, es.opacity * es.fill_op, &grads, current, bbox);
            }
            if es.stroke != Paint::None && es.stroke_w > 0.0 {
                let sp = ph.stroke(es.stroke_w, es.round_cap);
                draw(&mut cv, &ph, &sp, es.stroke, es.opacity * es.stroke_op, &grads, current, bbox);
            }
        }
    }
    Some(cv.into_vec())
}
