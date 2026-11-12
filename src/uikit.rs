//! A small immediate mode UI toolkit for the settings popover and the context
//! menu. Every widget runs twice per event: once to handle input (without a
//! canvas) and once to draw. Hover states fade in and out smoothly.

use crate::app::Fonts;
use crate::edit::Edit;
use crate::ffi::*;
use crate::gfx::*;
use crate::raster::*;
use crate::text::{self, Mask};
use crate::util::*;
use alloc::vec::Vec;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: u32,
    pub border: u32,
    pub surface: u32,
    pub surface_hot: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub danger: u32,
    pub track: u32,
}

impl Theme {
    pub fn dark(accent: u32) -> Theme {
        // Pick black or white text on the accent depending on its brightness.
        let l = (((accent >> 16) & 255) * 77 + ((accent >> 8) & 255) * 150 + (accent & 255) * 29) >> 8;
        Theme {
            bg: 0xFF1E1F24,
            border: 0x1CFFFFFF,
            surface: 0xFF2A2B31,
            surface_hot: 0xFF34353C,
            text: 0xFFECEDF0,
            muted: 0xFF8E909A,
            accent: accent | 0xFF00_0000,
            on_accent: if l > 160 { 0xFF16171A } else { 0xFFFFFFFF },
            danger: 0xFFFF5A52,
            track: 0xFF3A3B43,
        }
    }
}

/// Per-widget animated values (hover fades, toggle knobs).
pub struct Anim {
    v: Vec<(u32, f32, f32)>,
}
impl Anim {
    pub const fn new() -> Anim {
        Anim { v: Vec::new() }
    }
    /// Sets the target for `id` and returns its current value.
    pub fn get(&mut self, id: u32, target: f32) -> f32 {
        match self.v.iter_mut().find(|e| e.0 == id) {
            Some(e) => {
                e.2 = target;
                e.1
            }
            None => {
                self.v.push((id, target, target));
                target
            }
        }
    }
    pub fn step(&mut self, dt: f32) -> bool {
        let k = clamp(dt / 90.0, 0.0, 1.0);
        let mut busy = false;
        for e in self.v.iter_mut() {
            let d = e.2 - e.1;
            if abs(d) < 0.002 {
                e.1 = e.2;
            } else {
                e.1 += d * k.max(0.18);
                busy = true;
            }
        }
        busy
    }
    pub fn busy(&self) -> bool {
        self.v.iter().any(|e| abs(e.2 - e.1) >= 0.002)
    }
}

/// Text masks cached for the lifetime of a popup.
pub struct Texts {
    v: Vec<(u64, Mask)>,
}
impl Texts {
    pub const fn new() -> Texts {
        Texts { v: Vec::new() }
    }
    pub fn get(&mut self, fonts: &mut Fonts, face: &str, s: &str, px: i32, weight: i32) -> &Mask {
        let key = fnv(fnv(fnv(0, s.as_bytes()), &px.to_le_bytes()), &weight.to_le_bytes());
        if let Some(i) = self.v.iter().position(|e| e.0 == key) {
            return &self.v[i].1;
        }
        let f = fonts.get(face, px, weight);
        let m = text::render(&to_w(s), f, 2000, 1, false);
        self.v.push((key, m));
        &self.v.last().unwrap().1
    }
    /// Text wrapped to `max_w` pixels, up to `lines` lines.
    pub fn wrapped(&mut self, fonts: &mut Fonts, face: &str, s: &str, px: i32, weight: i32, max_w: i32, lines: i32) -> &Mask {
        let key = fnv(fnv(fnv(fnv(1, s.as_bytes()), &px.to_le_bytes()), &weight.to_le_bytes()), &max_w.to_le_bytes());
        if let Some(i) = self.v.iter().position(|e| e.0 == key) {
            return &self.v[i].1;
        }
        let f = fonts.get(face, px, weight);
        let m = text::render_left(&to_w(s), f, max_w, lines);
        self.v.push((key, m));
        &self.v.last().unwrap().1
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Ic {
    Gear,
    Open,
    Folder,
    Rename,
    Image,
    Reset,
    Out,
    Plus,
    Panel,
    Trash,
    Power,
    File,
    ChevronUp,
    ChevronDown,
    Close,
    Check,
    Palette,
    Clock,
    Info,
    GitHub,
    Refresh,
}

/// Icon coverage masks, drawn from small vector paths on a 24 unit grid.
pub struct Icons {
    v: Vec<(Ic, i32, Vec<u8>)>,
}
impl Icons {
    pub const fn new() -> Icons {
        Icons { v: Vec::new() }
    }
    pub fn get(&mut self, ic: Ic, s: i32) -> &[u8] {
        if let Some(i) = self.v.iter().position(|e| e.0 == ic && e.1 == s) {
            return &self.v[i].2;
        }
        self.v.push((ic, s, icon_mask(ic, s)));
        &self.v.last().unwrap().2
    }
}

/// Any toolkit icon as a coverage mask, for drawing outside a Ui.
pub fn icon_mask_pub(ic: Ic, s: i32) -> Vec<u8> {
    icon_mask(ic, s)
}

/// The settings button icon, used by the folder header.
pub fn gear_mask(s: i32) -> Vec<u8> {
    icon_mask(Ic::Gear, s)
}

/// The GitHub mark (from GitHub's Octicons, MIT license), drawn filled.
const GITHUB: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-1.6 -1.6 19.2 19.2"><path fill="#fff" d=""##,
    "M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z",
    r##""/></svg>"##
);

fn icon_mask(ic: Ic, s: i32) -> Vec<u8> {
    if let Ic::GitHub = ic {
        let px = crate::svg::render(GITHUB.as_bytes(), s, 0).unwrap_or_default();
        return px.iter().map(|c| (c >> 24) as u8).collect();
    }
    let k = s as f32 / 24.0;
    let mut ph = Path::new(Mat::scale(k, k));
    match ic {
        Ic::Gear => {
            // toothed outline around a small hub
            let teeth = 8;
            for i in 0..teeth * 4 {
                let a = (i as f32 / (teeth * 4) as f32) * 2.0 * PI - PI / 2.0;
                let r = if (i % 4) == 1 || (i % 4) == 2 { 9.6 } else { 7.4 };
                let q = p(12.0 + r * cos(a), 12.0 + r * sin(a));
                if i == 0 { ph.move_to(q) } else { ph.line_to(q) }
            }
            ph.close();
            ph.ellipse(12.0, 12.0, 3.0, 3.0);
        }
        Ic::Open => {
            ph.move_to(p(14.0, 4.0));
            ph.line_to(p(20.0, 4.0));
            ph.line_to(p(20.0, 10.0));
            ph.move_to(p(20.0, 4.0));
            ph.line_to(p(11.0, 13.0));
            ph.move_to(p(18.0, 14.0));
            ph.line_to(p(18.0, 19.0));
            ph.line_to(p(5.0, 19.0));
            ph.line_to(p(5.0, 6.0));
            ph.line_to(p(10.0, 6.0));
        }
        Ic::Folder => {
            ph.move_to(p(3.0, 7.0));
            ph.line_to(p(3.0, 18.0));
            ph.line_to(p(21.0, 18.0));
            ph.line_to(p(21.0, 8.5));
            ph.line_to(p(12.0, 8.5));
            ph.line_to(p(10.0, 6.0));
            ph.line_to(p(3.0, 6.0));
            ph.close();
        }
        Ic::Rename => {
            ph.move_to(p(4.0, 20.0));
            ph.line_to(p(8.0, 20.0));
            ph.line_to(p(19.0, 9.0));
            ph.line_to(p(15.0, 5.0));
            ph.line_to(p(4.0, 16.0));
            ph.close();
            ph.move_to(p(13.0, 7.0));
            ph.line_to(p(17.0, 11.0));
        }
        Ic::Image => {
            ph.rrect(3.5, 4.5, 17.0, 15.0, 2.0, 2.0);
            ph.move_to(p(4.0, 17.0));
            ph.line_to(p(9.0, 12.0));
            ph.line_to(p(13.0, 16.0));
            ph.line_to(p(16.0, 13.0));
            ph.line_to(p(20.0, 17.0));
            ph.ellipse(15.5, 8.5, 1.3, 1.3);
        }
        Ic::Reset => {
            ph.move_to(p(4.5, 12.0));
            ph.arc_to(7.5, 7.5, 0.0, true, true, p(8.0, 5.5));
            ph.move_to(p(4.0, 4.0));
            ph.line_to(p(4.0, 9.0));
            ph.line_to(p(9.0, 9.0));
        }
        Ic::Refresh => {
            // a circle that ends in an arrow, turning clockwise
            ph.move_to(p(20.0, 12.0));
            ph.arc_to(8.0, 8.0, 0.0, true, true, p(12.0, 4.0));
            ph.arc_to(8.0, 8.0, 0.0, false, true, p(17.66, 6.34));
            ph.line_to(p(20.0, 8.5));
            ph.move_to(p(20.0, 3.5));
            ph.line_to(p(20.0, 8.5));
            ph.line_to(p(15.0, 8.5));
        }
        Ic::Out => {
            ph.move_to(p(12.0, 4.0));
            ph.line_to(p(12.0, 14.0));
            ph.move_to(p(8.0, 10.0));
            ph.line_to(p(12.0, 14.0));
            ph.line_to(p(16.0, 10.0));
            ph.move_to(p(5.0, 19.5));
            ph.line_to(p(19.0, 19.5));
        }
        Ic::Plus => {
            ph.move_to(p(12.0, 5.0));
            ph.line_to(p(12.0, 19.0));
            ph.move_to(p(5.0, 12.0));
            ph.line_to(p(19.0, 12.0));
        }
        Ic::Panel => {
            ph.rrect(3.5, 5.0, 17.0, 14.0, 2.0, 2.0);
            ph.move_to(p(3.5, 9.5));
            ph.line_to(p(20.5, 9.5));
        }
        Ic::Trash => {
            ph.move_to(p(4.0, 7.0));
            ph.line_to(p(20.0, 7.0));
            ph.move_to(p(9.0, 7.0));
            ph.line_to(p(9.0, 4.0));
            ph.line_to(p(15.0, 4.0));
            ph.line_to(p(15.0, 7.0));
            ph.move_to(p(6.0, 7.0));
            ph.line_to(p(7.0, 20.0));
            ph.line_to(p(17.0, 20.0));
            ph.line_to(p(18.0, 7.0));
        }
        Ic::Power => {
            ph.move_to(p(12.0, 3.5));
            ph.line_to(p(12.0, 11.0));
            ph.move_to(p(7.0, 6.5));
            ph.arc_to(7.5, 7.5, 0.0, true, false, p(17.0, 6.5));
        }
        Ic::ChevronUp | Ic::ChevronDown => {
            let (a, b) = if ic == Ic::ChevronUp { (14.5, 9.5) } else { (9.5, 14.5) };
            ph.move_to(p(7.0, a));
            ph.line_to(p(12.0, b));
            ph.line_to(p(17.0, a));
        }
        Ic::Close => {
            ph.move_to(p(7.0, 7.0));
            ph.line_to(p(17.0, 17.0));
            ph.move_to(p(17.0, 7.0));
            ph.line_to(p(7.0, 17.0));
        }
        Ic::Check => {
            ph.move_to(p(5.5, 12.5));
            ph.line_to(p(10.0, 17.0));
            ph.line_to(p(18.5, 7.5));
        }
        Ic::Palette => {
            ph.move_to(p(12.0, 3.5));
            ph.arc_to(8.5, 8.5, 0.0, true, false, p(13.5, 20.4));
            ph.line_to(p(13.0, 17.0));
            ph.line_to(p(16.5, 16.0));
            ph.arc_to(4.0, 4.0, 0.0, false, false, p(20.5, 12.0));
            ph.arc_to(8.5, 8.5, 0.0, false, false, p(12.0, 3.5));
            ph.ellipse(8.0, 10.0, 1.0, 1.0);
            ph.ellipse(12.0, 7.5, 1.0, 1.0);
            ph.ellipse(16.0, 10.0, 1.0, 1.0);
        }
        Ic::Clock => {
            ph.ellipse(12.0, 12.0, 8.5, 8.5);
            ph.move_to(p(12.0, 7.0));
            ph.line_to(p(12.0, 12.0));
            ph.line_to(p(15.5, 14.0));
        }
        Ic::GitHub => {}
        Ic::Info => {
            ph.ellipse(12.0, 12.0, 8.5, 8.5);
            ph.move_to(p(12.0, 11.0));
            ph.line_to(p(12.0, 16.5));
            ph.ellipse(12.0, 7.8, 0.4, 0.4);
        }
        Ic::File => {
            ph.move_to(p(6.0, 3.5));
            ph.line_to(p(14.0, 3.5));
            ph.line_to(p(18.5, 8.0));
            ph.line_to(p(18.5, 20.5));
            ph.line_to(p(6.0, 20.5));
            ph.close();
            ph.move_to(p(13.5, 3.5));
            ph.line_to(p(13.5, 8.5));
            ph.line_to(p(18.5, 8.5));
        }
    }
    let st = ph.stroke(1.7, true);
    rasterize(&st, s as usize, s as usize).a
}

/// Mouse state for one event.
#[derive(Clone, Copy, Default)]
pub struct Input {
    pub mx: f32,
    pub my: f32,
    pub inside: bool,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub dbl: bool,
}

pub struct Ui<'a> {
    pub cv: Option<&'a mut Canvas>,
    pub sc: f32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub inp: Input,
    pub active: &'a mut u32,
    pub anim: &'a mut Anim,
    pub texts: &'a mut Texts,
    pub icons: &'a mut Icons,
    pub fonts: &'a mut Fonts,
    pub face: &'a str,
    pub th: Theme,
    pub cursor: usize,
    /// Visible vertical range, used to ignore clicks on scrolled-out widgets.
    pub view: (f32, f32),
    /// Id whose press just ended this event (sliders commit on release).
    pub released_id: u32,
}

pub fn fmt_num(v: i32, unit: &str) -> alloc::string::String {
    let mut s = alloc::string::String::new();
    fmt_int(v as i64, &mut s);
    s.push_str(unit);
    s
}

impl<'a> Ui<'a> {
    pub fn s(&self, v: f32) -> f32 {
        v * self.sc
    }

    /// Hit testing for a widget rectangle. Returns (hot, clicked).
    pub fn interact(&mut self, id: u32, x: f32, y: f32, w: f32, h: f32) -> (bool, bool) {
        let i = self.inp;
        let visible = i.my >= self.view.0 && i.my < self.view.1;
        let hot = i.inside && visible && i.mx >= x && i.mx < x + w && i.my >= y && i.my < y + h;
        if hot && i.pressed {
            *self.active = id;
        }
        let clicked = i.released && *self.active == id && hot;
        if i.released && *self.active == id {
            self.released_id = id;
        }
        (hot, clicked)
    }

    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: u32, alpha: f32) {
        if let Some(cv) = self.cv.as_deref_mut() {
            cv.rrect(x, y, w, h, r, c, c, (clamp(alpha, 0.0, 1.0) * 255.0) as u32, 1.0);
        }
    }
    fn ring(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, c: u32) {
        if let Some(cv) = self.cv.as_deref_mut() {
            cv.rrect_stroke(x, y, w, h, r, t, c, c, 255);
        }
    }

    /// Text size in pixels for a string, without drawing it.
    pub fn measure(&mut self, s: &str, px: f32, weight: i32) -> (i32, i32) {
        let m = self.texts.get(self.fonts, self.face, s, (px * self.sc) as i32, weight);
        (m.w, m.h)
    }

    /// Draws text with its top-left at (x, y), or right aligned when `right` is set.
    pub fn text(&mut self, s: &str, x: f32, y: f32, px: f32, weight: i32, color: u32, right: bool) -> f32 {
        let sz = (px * self.sc) as i32;
        let m = self.texts.get(self.fonts, self.face, s, sz, weight);
        let (w, h) = (m.w, m.h);
        if let Some(cv) = self.cv.as_deref_mut() {
            let m = self.texts.get(self.fonts, self.face, s, sz, weight);
            let xx = if right { x - w as f32 } else { x };
            cv.mask(&m.a, m.w, m.h, xx as i32, y as i32, pm(color, 255));
        }
        let _ = h;
        w as f32
    }
    fn text_h(&mut self, px: f32) -> f32 {
        self.measure("Ag", px, FW_NORMAL).1 as f32
    }
    /// Text vertically centered in a band of height `h` starting at `y`.
    pub fn text_mid(&mut self, s: &str, x: f32, y: f32, h: f32, px: f32, weight: i32, color: u32, right: bool) -> f32 {
        let th = self.text_h(px);
        self.text(s, x, y + (h - th) / 2.0, px, weight, color, right)
    }

    pub fn icon(&mut self, ic: Ic, x: f32, y: f32, size: f32, color: u32) {
        let s = size as i32;
        if self.cv.is_none() {
            return;
        }
        let m = self.icons.get(ic, s).to_vec();
        if let Some(cv) = self.cv.as_deref_mut() {
            cv.mask(&m, s, s, x as i32, y as i32, pm(color, 255));
        }
    }

    // ------------------------------------------------------------ widgets
    pub fn space(&mut self, h: f32) {
        self.y += self.s(h);
    }

    pub fn section(&mut self, title: &str) {
        self.y += self.s(14.0);
        let x = self.x;
        let y = self.y;
        let c = self.th.muted;
        self.text(title, x, y, 11.5, FW_SEMIBOLD, c, false);
        self.y += self.s(22.0);
    }

    /// Plain label on its own line.
    pub fn label(&mut self, s: &str) {
        let (x, y) = (self.x, self.y);
        let c = self.th.text;
        self.text(s, x, y, 13.0, FW_NORMAL, c, false);
        self.y += self.s(22.0);
    }

    pub fn segmented(&mut self, id: u32, opts: &[&str], sel: &mut usize) -> bool {
        let h = self.s(32.0);
        let (x, y, w) = (self.x, self.y, self.w);
        let pad = self.s(3.0);
        self.fill(x, y, w, h, self.s(9.0), self.th.surface, 1.0);
        let n = opts.len().max(1) as f32;
        let sw = (w - pad * 2.0) / n;
        let mut changed = false;
        for (k, o) in opts.iter().enumerate() {
            let sx = x + pad + sw * k as f32;
            let (hot, clicked) = self.interact(id + k as u32, sx, y, sw, h);
            if clicked && *sel != k {
                *sel = k;
                changed = true;
            }
            let on = *sel == k;
            let ha = self.anim.get(id + k as u32, if hot && !on { 1.0 } else { 0.0 });
            if on {
                self.fill(sx, y + pad, sw, h - pad * 2.0, self.s(7.0), self.th.accent, 1.0);
            } else if ha > 0.0 {
                self.fill(sx, y + pad, sw, h - pad * 2.0, self.s(7.0), self.th.surface_hot, ha);
            }
            let c = if on { self.th.on_accent } else { self.th.text };
            let (tw, _) = self.measure(o, 12.5, FW_SEMIBOLD);
            self.text_mid(o, sx + (sw - tw as f32) / 2.0, y, h, 12.5, FW_SEMIBOLD, c, false);
            if hot {
                self.cursor = IDC_HAND;
            }
        }
        self.y += h + self.s(10.0);
        changed
    }

    pub fn toggle(&mut self, id: u32, label: &str, val: &mut bool) -> bool {
        let h = self.s(34.0);
        let (x, y, w) = (self.x, self.y, self.w);
        let (hot, clicked) = self.interact(id, x, y, w, h);
        if clicked {
            *val = !*val;
        }
        let c = self.th.text;
        self.text_mid(label, x, y, h, 13.0, FW_NORMAL, c, false);
        let (tw, th) = (self.s(38.0), self.s(22.0));
        let tx = x + w - tw;
        let ty = y + (h - th) / 2.0;
        let k = self.anim.get(id, if *val { 1.0 } else { 0.0 });
        let track = mix(self.th.track, self.th.accent, k);
        self.fill(tx, ty, tw, th, th / 2.0, track, 1.0);
        let ks = th - self.s(6.0);
        let kx = lerp(tx + self.s(3.0), tx + tw - ks - self.s(3.0), ease_in_out_cubic(k));
        let knob = mix(0xFFD8D9DE, self.th.on_accent, k);
        self.fill(kx, ty + self.s(3.0), ks, ks, ks / 2.0, knob, 1.0);
        if hot {
            self.cursor = IDC_HAND;
        }
        self.y += h + self.s(2.0);
        clicked
    }

    /// Slider with a label and value. Returns the new value while dragging.
    pub fn slider(&mut self, id: u32, label: &str, val: f32, min: f32, max: f32, step: f32, unit: &str) -> Option<f32> {
        let (x, y, w) = (self.x, self.y, self.w);
        let c = self.th.text;
        let m = self.th.muted;
        self.text(label, x, y, 13.0, FW_NORMAL, c, false);
        let vs = fmt_num(round(val) as i32, unit);
        self.text(&vs, x + w, y, 12.5, FW_NORMAL, m, true);
        let ty = y + self.s(22.0);
        let th = self.s(22.0);
        let (hot, _) = self.interact(id, x - self.s(4.0), ty, w + self.s(8.0), th);
        let mut out = None;
        if *self.active == id && self.inp.down {
            let f = clamp((self.inp.mx - x) / w, 0.0, 1.0);
            let mut v = min + f * (max - min);
            if step > 0.0 {
                v = round(v / step) * step;
            }
            if v != val {
                out = Some(v);
            }
        }
        let cur = out.unwrap_or(val);
        let f = clamp((cur - min) / (max - min), 0.0, 1.0);
        let tr = self.s(4.0);
        let cy = ty + th / 2.0;
        self.fill(x, cy - tr / 2.0, w, tr, tr / 2.0, self.th.track, 1.0);
        self.fill(x, cy - tr / 2.0, w * f, tr, tr / 2.0, self.th.accent, 1.0);
        let ha = self.anim.get(id, if hot || *self.active == id { 1.0 } else { 0.0 });
        let kr = self.s(7.0) + self.s(1.5) * ha;
        let kx = x + w * f;
        if ha > 0.0 {
            let halo = self.s(12.0);
            self.fill(kx - halo, cy - halo, halo * 2.0, halo * 2.0, halo, self.th.accent, 0.18 * ha);
        }
        self.fill(kx - kr, cy - kr, kr * 2.0, kr * 2.0, kr, 0xFFF4F5F7, 1.0);
        if hot {
            self.cursor = IDC_HAND;
        }
        self.y = ty + th + self.s(8.0);
        out
    }

    /// Row of color circles. Returns Some(color) for a preset, Some(0) for "custom".
    pub fn swatches(&mut self, id: u32, colors: &[u32], sel: u32, custom: bool) -> Option<u32> {
        let d = self.s(22.0);
        let total = colors.len() + custom as usize;
        // spread the circles over the full width, wrapping only when they don't fit
        let min_gap = self.s(6.0);
        let per_row = (((self.w + min_gap) / (d + min_gap)) as usize).clamp(1, total.max(1));
        let gap = if per_row > 1 { ((self.w - per_row as f32 * d) / (per_row - 1) as f32).min(self.s(12.0)) } else { 0.0 };
        let mut res = None;
        for k in 0..total {
            let (row, col) = (k / per_row, k % per_row);
            let x = self.x + col as f32 * (d + gap);
            let y = self.y + row as f32 * (d + gap);
            let (hot, clicked) = self.interact(id + k as u32, x, y, d, d);
            let ha = self.anim.get(id + k as u32, if hot { 1.0 } else { 0.0 });
            let grow = self.s(2.0) * ha;
            let is_custom = k == colors.len();
            let selected = !is_custom && (colors[k] & 0x00FF_FFFF) == (sel & 0x00FF_FFFF);
            if selected {
                self.ring(x - self.s(3.0), y - self.s(3.0), d + self.s(6.0), d + self.s(6.0), d, self.s(2.0), self.th.text);
            }
            if is_custom {
                // a little color wheel with a plus
                if let Some(cv) = self.cv.as_deref_mut() {
                    let (cx, cy, r) = (x + d / 2.0, y + d / 2.0, d / 2.0 + grow / 2.0);
                    let (x0, y0) = ((cx - r) as i32, (cy - r) as i32);
                    for py in y0..(cy + r) as i32 + 1 {
                        for px in x0..(cx + r) as i32 + 1 {
                            let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                            let dist = sqrt(dx * dx + dy * dy);
                            let cov = clamp(r - dist + 0.5, 0.0, 1.0);
                            if cov <= 0.0 || px < cv.clip[0] || py < cv.clip[1] || px >= cv.clip[2] || py >= cv.clip[3] {
                                continue;
                            }
                            let hue = fract(atan2(dy, dx) / (2.0 * PI) + 1.0);
                            let c = hsv(hue, 0.75, 1.0);
                            let i = (py * cv.w + px) as usize;
                            let pxs = cv.pixels();
                            pxs[i] = over(pxs[i], scale(pm(c, 255), (cov * 256.0) as u32));
                        }
                    }
                }
                let ic = self.s(12.0);
                self.fill(x + (d - self.s(14.0)) / 2.0, y + (d - self.s(14.0)) / 2.0, self.s(14.0), self.s(14.0), self.s(7.0), 0xFF1E1F24, 0.85);
                let cx = x + (d - ic) / 2.0;
                self.icon(Ic::Plus, cx, y + (d - ic) / 2.0, ic, 0xFFFFFFFF);
            } else {
                let c = colors[k] | 0xFF00_0000;
                self.fill(x - grow / 2.0, y - grow / 2.0, d + grow, d + grow, (d + grow) / 2.0, c, 1.0);
                self.ring(x - grow / 2.0, y - grow / 2.0, d + grow, d + grow, (d + grow) / 2.0, self.s(1.0), 0x26FFFFFF);
            }
            if hot {
                self.cursor = IDC_HAND;
            }
            if clicked {
                res = Some(if is_custom { 0 } else { colors[k] | 0xFF00_0000 });
            }
        }
        let rows = (total + per_row - 1) / per_row;
        self.y += rows as f32 * (d + gap) + self.s(4.0);
        res
    }

    /// Hue, saturation and value picker. Returns true when the color changed.
    pub fn hsv_picker(&mut self, id: u32, h: &mut f32, s: &mut f32, v: &mut f32) -> bool {
        let (x, y, w) = (self.x, self.y, self.w);
        let bh = self.s(118.0);
        let mut changed = false;
        let (_hot, _) = self.interact(id, x, y, w, bh);
        if *self.active == id && self.inp.down {
            *s = clamp((self.inp.mx - x) / w, 0.0, 1.0);
            *v = 1.0 - clamp((self.inp.my - y) / bh, 0.0, 1.0);
            changed = true;
        }
        let hy = y + bh + self.s(10.0);
        let hh = self.s(14.0);
        let (_hhot, _) = self.interact(id + 1, x, hy - self.s(4.0), w, hh + self.s(8.0));
        if *self.active == id + 1 && self.inp.down {
            *h = clamp((self.inp.mx - x) / w, 0.0, 0.9999);
            changed = true;
        }
        if let Some(cv) = self.cv.as_deref_mut() {
            let r = 6.0 * self.sc;
            // saturation/value square with rounded corners
            for py in y as i32..(y + bh) as i32 {
                if py < cv.clip[1] || py >= cv.clip[3] {
                    continue;
                }
                let vv = 1.0 - (py as f32 + 0.5 - y) / bh;
                for px in x as i32..(x + w) as i32 {
                    let ss = (px as f32 + 0.5 - x) / w;
                    let c = hsv(*h, ss, vv);
                    let i = (py * cv.w + px) as usize;
                    cv.pixels()[i] = pm(c, 255);
                }
            }
            // round the corners by painting the popup background over them
            let bg = 0xFF1E1F24;
            for (cx, cy) in [(x, y), (x + w - r, y), (x, y + bh - r), (x + w - r, y + bh - r)] {
                for py in cy as i32..(cy + r) as i32 {
                    for px in cx as i32..(cx + r) as i32 {
                        let ox = if px as f32 <= x + r { x + r } else { x + w - r };
                        let oy = if py as f32 <= y + r { y + r } else { y + bh - r };
                        let (dx, dy) = (px as f32 + 0.5 - ox, py as f32 + 0.5 - oy);
                        let out = clamp(sqrt(dx * dx + dy * dy) - r + 0.5, 0.0, 1.0);
                        if out > 0.0 && py >= cv.clip[1] && py < cv.clip[3] {
                            let i = (py * cv.w + px) as usize;
                            let pxs = cv.pixels();
                            pxs[i] = over(pxs[i], scale(pm(bg, 255), (out * 256.0) as u32));
                        }
                    }
                }
            }
            // hue bar
            for py in hy as i32..(hy + hh) as i32 {
                if py < cv.clip[1] || py >= cv.clip[3] {
                    continue;
                }
                for px in x as i32..(x + w) as i32 {
                    let c = hsv((px as f32 + 0.5 - x) / w, 1.0, 1.0);
                    let i = (py * cv.w + px) as usize;
                    cv.pixels()[i] = pm(c, 255);
                }
            }
        }
        // markers
        let mx = x + *s * w;
        let my = y + (1.0 - *v) * bh;
        let mr = self.s(7.0);
        self.ring(mx - mr, my - mr, mr * 2.0, mr * 2.0, mr, self.s(2.0), 0xFFFFFFFF);
        let hx = x + *h * w;
        self.ring(hx - self.s(4.0), hy - self.s(3.0), self.s(8.0), hh + self.s(6.0), self.s(3.0), self.s(2.0), 0xFFFFFFFF);
        self.y = hy + hh + self.s(12.0);
        changed
    }

    pub fn button(&mut self, id: u32, label: &str, ic: Option<Ic>, style: u8, x: f32, w: f32) -> bool {
        // style 0 = normal, 1 = accent, 2 = danger outline, 3 = danger solid
        let h = self.s(34.0);
        let y = self.y;
        let (hot, clicked) = self.interact(id, x, y, w, h);
        let ha = self.anim.get(id, if hot { 1.0 } else { 0.0 });
        let (bg, fg) = match style {
            1 => (self.th.accent, self.th.on_accent),
            2 => (mix(self.th.surface, self.th.surface_hot, ha), self.th.danger),
            3 => (self.th.danger, 0xFFFFFFFF),
            _ => (mix(self.th.surface, self.th.surface_hot, ha), self.th.text),
        };
        self.fill(x, y, w, h, self.s(9.0), bg, 1.0);
        if style == 1 || style == 3 {
            self.fill(x, y, w, h, self.s(9.0), 0xFFFFFFFF, 0.08 * ha);
        }
        let (tw, _) = self.measure(label, 13.0, FW_SEMIBOLD);
        let isz = self.s(16.0);
        let gap = if ic.is_some() { isz + self.s(8.0) } else { 0.0 };
        let sx = x + (w - tw as f32 - gap) / 2.0;
        if let Some(ic) = ic {
            self.icon(ic, sx, y + (h - isz) / 2.0, isz, fg);
        }
        self.text_mid(label, sx + gap, y, h, 13.0, FW_SEMIBOLD, fg, false);
        if hot {
            self.cursor = IDC_HAND;
        }
        clicked
    }

    /// Text field. Returns true when clicked (so the caller can focus it).
    pub fn field(&mut self, id: u32, ed: &mut Edit, focused: bool, f: HFONT) -> bool {
        let h = self.s(34.0);
        let (x, y, w) = (self.x, self.y, self.w);
        let (hot, _) = self.interact(id, x, y, w, h);
        let pressed_here = hot && self.inp.pressed;
        let ha = self.anim.get(id, if focused { 1.0 } else if hot { 0.5 } else { 0.0 });
        self.fill(x, y, w, h, self.s(9.0), mix(self.th.surface, self.th.surface_hot, ha * 0.6), 1.0);
        if focused {
            self.ring(x, y, w, h, self.s(9.0), self.s(1.5), self.th.accent);
        }
        let pad = self.s(11.0);
        if pressed_here {
            let at = ed.hit(f, x + pad, self.inp.mx);
            if self.inp.dbl {
                ed.select_word(at);
            } else {
                ed.caret = at;
                if !(unsafe { GetKeyState(VK_SHIFT) } < 0) {
                    ed.anchor = at;
                }
                ed.selecting = true;
            }
        } else if focused && ed.selecting && self.inp.down && *self.active == id {
            ed.caret = ed.hit(f, x + pad, self.inp.mx);
        }
        if self.inp.released {
            ed.selecting = false;
        }
        let th = self.th;
        if let Some(cv) = self.cv.as_deref_mut() {
            ed.draw(cv, f, (x + pad, y, w - pad * 2.0, h), th.text, (th.accent & 0x00FF_FFFF) | 0x6000_0000, focused);
        }
        if hot {
            self.cursor = IDC_IBEAM;
        }
        self.y += h + self.s(8.0);
        pressed_here
    }

    /// Context menu entry. Returns true when clicked.
    pub fn menu_item(&mut self, id: u32, ic: Ic, label: &str, danger: bool, keyboard: bool) -> bool {
        let h = self.s(34.0);
        let (x, y, w) = (self.x, self.y, self.w);
        let (hot, clicked) = self.interact(id, x, y, w, h);
        let ha = self.anim.get(id, if hot || keyboard { 1.0 } else { 0.0 });
        if ha > 0.0 {
            self.fill(x, y, w, h, self.s(7.0), self.th.surface_hot, ha);
        }
        let fg = if danger { self.th.danger } else { self.th.text };
        let isz = self.s(16.0);
        let icol = if danger { self.th.danger } else { mix(self.th.muted, self.th.text, ha) };
        self.icon(ic, x + self.s(10.0), y + (h - isz) / 2.0, isz, icol);
        self.text_mid(label, x + self.s(36.0), y, h, 13.0, FW_NORMAL, fg, false);
        self.y += h;
        clicked
    }

    /// A paragraph that wraps to the content width.
    pub fn paragraph(&mut self, s: &str, px: f32, color: u32) {
        let (x, y, w) = (self.x, self.y, self.w);
        let m = self.texts.wrapped(self.fonts, self.face, s, (px * self.sc) as i32, FW_NORMAL, w as i32, 6);
        let h = m.h;
        if let Some(cv) = self.cv.as_deref_mut() {
            let m = self.texts.wrapped(self.fonts, self.face, s, (px * self.sc) as i32, FW_NORMAL, w as i32, 6);
            cv.mask(&m.a, m.w, m.h, x as i32, y as i32, pm(color, 255));
        }
        self.y += h as f32 + self.s(10.0);
    }

    /// One option in a single-choice list, with a check mark when selected.
    pub fn choice(&mut self, id: u32, label: &str, selected: bool) -> bool {
        let h = self.s(34.0);
        let (x, y, w) = (self.x, self.y, self.w);
        let (hot, clicked) = self.interact(id, x, y, w, h);
        let ha = self.anim.get(id, if hot { 1.0 } else { 0.0 });
        if selected {
            self.fill(x, y, w, h, self.s(8.0), self.th.surface, 1.0);
        }
        if ha > 0.0 {
            self.fill(x, y, w, h, self.s(8.0), self.th.surface_hot, ha * 0.8);
        }
        let c = self.th.text;
        self.text_mid(label, x + self.s(12.0), y, h, 13.0, if selected { FW_SEMIBOLD } else { FW_NORMAL }, c, false);
        if selected {
            let isz = self.s(16.0);
            let a = self.th.accent;
            self.icon(Ic::Check, x + w - isz - self.s(12.0), y + (h - isz) / 2.0, isz, a);
        }
        if hot {
            self.cursor = IDC_HAND;
        }
        self.y += h + self.s(2.0);
        clicked
    }

    /// Sidebar navigation entry.
    pub fn nav(&mut self, id: u32, ic: Ic, label: &str, selected: bool) -> bool {
        let h = self.s(36.0);
        let (x, y, w) = (self.x, self.y, self.w);
        let (hot, clicked) = self.interact(id, x, y, w, h);
        let ha = self.anim.get(id, if hot && !selected { 1.0 } else { 0.0 });
        if selected {
            self.fill(x, y, w, h, self.s(8.0), self.th.surface_hot, 1.0);
            let a = self.th.accent;
            self.fill(x, y + self.s(9.0), self.s(3.0), h - self.s(18.0), self.s(1.5), a, 1.0);
        } else if ha > 0.0 {
            self.fill(x, y, w, h, self.s(8.0), self.th.surface, ha);
        }
        let isz = self.s(16.0);
        let ic_col = if selected { self.th.text } else { self.th.muted };
        self.icon(ic, x + self.s(12.0), y + (h - isz) / 2.0, isz, ic_col);
        let c = if selected { self.th.text } else { mix(self.th.muted, self.th.text, 0.6) };
        self.text_mid(label, x + self.s(38.0), y, h, 13.0, if selected { FW_SEMIBOLD } else { FW_NORMAL }, c, false);
        if hot {
            self.cursor = IDC_HAND;
        }
        self.y += h + self.s(2.0);
        clicked
    }

    /// Small round icon button. Returns true when clicked.
    pub fn icon_button(&mut self, id: u32, ic: Ic, x: f32, y: f32, size: f32) -> bool {
        let (hot, clicked) = self.interact(id, x, y, size, size);
        let ha = self.anim.get(id, if hot { 1.0 } else { 0.0 });
        if ha > 0.0 {
            self.fill(x, y, size, size, size / 2.0, self.th.surface_hot, ha);
        }
        let isz = self.s(16.0);
        let c = mix(self.th.muted, self.th.text, ha);
        self.icon(ic, x + (size - isz) / 2.0, y + (size - isz) / 2.0, isz, c);
        if hot {
            self.cursor = IDC_HAND;
        }
        clicked
    }

    pub fn separator(&mut self) {
        let y = self.y + self.s(4.5);
        let (x, w) = (self.x + self.s(8.0), self.w - self.s(16.0));
        self.fill(x, y, w, self.s(1.0), 0.0, self.th.border, 1.0);
        self.y += self.s(10.0);
    }
}

/// HSV (all 0..1) to opaque ARGB.
pub fn hsv(h: f32, s: f32, v: f32) -> u32 {
    let h6 = fract(h) * 6.0;
    let i = floor(h6) as i32;
    let f = h6 - i as f32;
    let (p_, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p_),
        1 => (q, v, p_),
        2 => (p_, v, t),
        3 => (p_, q, v),
        4 => (t, p_, v),
        _ => (v, p_, q),
    };
    let c = |x: f32| (clamp(x, 0.0, 1.0) * 255.0 + 0.5) as u32;
    0xFF00_0000 | c(r) << 16 | c(g) << 8 | c(b)
}

/// Opaque ARGB to HSV (all 0..1).
pub fn to_hsv(c: u32) -> (f32, f32, f32) {
    let r = ((c >> 16) & 255) as f32 / 255.0;
    let g = ((c >> 8) & 255) as f32 / 255.0;
    let b = (c & 255) as f32 / 255.0;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        fract(((g - b) / d) / 6.0 + 1.0)
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if mx == 0.0 { 0.0 } else { d / mx };
    (h, s, mx)
}

pub fn hex(c: u32) -> alloc::string::String {
    const H: &[u8] = b"0123456789ABCDEF";
    let mut s = alloc::string::String::from("#");
    for sh in [20, 16, 12, 8, 4, 0] {
        s.push(H[((c >> sh) & 15) as usize] as char);
    }
    s
}
