//! System fonts through fontconfig (fc-match), drawn with ab_glyph.
//!
//! A font is the face fontconfig picks for the requested name and weight, plus
//! fallbacks found per character (Chinese, Cyrillic, symbols...). Files are
//! memory-mapped, so even large CJK collections cost little memory.

use ab_glyph::{Font as _, FontRef, Glyph, PxScale, ScaleFont as _};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

pub struct FontFile {
    // `font` borrows `_map`; fields drop in order, so it goes first.
    font: FontRef<'static>,
    _map: memmap2::Mmap,
}

impl FontFile {
    fn open(path: &str, index: u32) -> Option<Rc<FontFile>> {
        let key = (path.to_string(), index);
        let cache = files();
        if let Some(f) = cache.get(&key).and_then(Weak::upgrade) {
            return Some(f);
        }
        let file = std::fs::File::open(path).ok()?;
        let map = unsafe { memmap2::Mmap::map(&file).ok()? };
        let bytes: &'static [u8] = unsafe { core::slice::from_raw_parts(map.as_ptr(), map.len()) };
        let font = FontRef::try_from_slice_and_index(bytes, index).ok()?;
        let f = Rc::new(FontFile { font, _map: map });
        cache.insert(key, Rc::downgrade(&f));
        Some(f)
    }
}

fn files() -> &'static mut HashMap<(String, u32), Weak<FontFile>> {
    static mut F: Option<HashMap<(String, u32), Weak<FontFile>>> = None;
    unsafe { (*core::ptr::addr_of_mut!(F)).get_or_insert_with(HashMap::new) }
}

/// fontconfig's weight scale from a Windows weight.
fn fc_weight(w: i32) -> i32 {
    match w {
        ..=150 => 0,
        151..=250 => 40,
        251..=350 => 50,
        351..=450 => 80,
        451..=550 => 100,
        551..=650 => 180,
        651..=750 => 200,
        751..=850 => 205,
        _ => 210,
    }
}

fn lang_hint() -> &'static str {
    match crate::i18n::current() {
        "zh-Hans" => ":lang=zh-cn",
        "zh-Hant" => ":lang=zh-tw",
        "ru" => ":lang=ru",
        _ => "",
    }
}

fn fc_match(pattern: &str) -> Option<(String, u32)> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}|%{index}", pattern]).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let (file, idx) = s.trim().rsplit_once('|')?;
    (!file.is_empty()).then(|| (file.to_string(), idx.parse().unwrap_or(0)))
}

/// Fonts that are very likely present when fontconfig isn't.
const KNOWN: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];

fn escape(face: &str) -> String {
    face.replace('\\', "\\\\").replace('-', "\\-").replace(':', "\\:").replace(',', "\\,")
}

pub struct Font {
    pub px: f32,
    weight: i32,
    primary: Rc<FontFile>,
    fallbacks: RefCell<Vec<Rc<FontFile>>>,
    by_char: RefCell<HashMap<char, usize>>,
}

/// Fallback file per character, shared by every font and remembered for the run.
fn fallback_for(c: char, weight: i32) -> Option<(String, u32)> {
    static mut M: Option<HashMap<(char, i32), Option<(String, u32)>>> = None;
    let m = unsafe { (*core::ptr::addr_of_mut!(M)).get_or_insert_with(HashMap::new) };
    m.entry((c, weight))
        .or_insert_with(|| fc_match(&format!("sans-serif:weight={}:charset={:x}{}", fc_weight(weight), c as u32, lang_hint())))
        .clone()
}

impl Font {
    pub fn new(face: &str, px: i32, weight: i32) -> Option<Font> {
        let pat = format!("{}:weight={}{}", escape(if face.is_empty() { "sans-serif" } else { face }), fc_weight(weight), lang_hint());
        // Fonts are dropped whenever Crow goes idle, so remember the lookups.
        static mut M: Option<HashMap<String, Option<(String, u32)>>> = None;
        let m = unsafe { (*core::ptr::addr_of_mut!(M)).get_or_insert_with(HashMap::new) };
        let found = m.entry(pat.clone()).or_insert_with(|| fc_match(&pat)).clone();
        let primary = found
            .and_then(|(f, i)| FontFile::open(&f, i))
            .or_else(|| KNOWN.iter().find_map(|p| FontFile::open(p, 0)))?;
        Some(Font { px: px.max(1) as f32, weight, primary, fallbacks: RefCell::new(Vec::new()), by_char: RefCell::new(HashMap::new()) })
    }

    fn scale_for(&self, f: &FontRef<'static>) -> PxScale {
        // A negative height in CreateFontW is the em size, which is what Crow asks for.
        let upem = f.units_per_em().unwrap_or(1000.0);
        PxScale::from(self.px * f.height_unscaled() / upem)
    }

    /// Which file draws `c`: 0 is the primary face, then the fallbacks.
    fn pick(&self, c: char) -> usize {
        if self.primary.font.glyph_id(c).0 != 0 || c.is_control() {
            return 0;
        }
        if let Some(&i) = self.by_char.borrow().get(&c) {
            return i;
        }
        let mut i = 0;
        for (k, f) in self.fallbacks.borrow().iter().enumerate() {
            if f.font.glyph_id(c).0 != 0 {
                i = k + 1;
                break;
            }
        }
        if i == 0 {
            if let Some((p, idx)) = fallback_for(c, self.weight) {
                if let Some(f) = FontFile::open(&p, idx) {
                    if f.font.glyph_id(c).0 != 0 {
                        let mut fb = self.fallbacks.borrow_mut();
                        fb.push(f);
                        i = fb.len();
                    }
                }
            }
        }
        self.by_char.borrow_mut().insert(c, i);
        i
    }

    fn with<R>(&self, i: usize, f: impl FnOnce(&FontRef<'static>) -> R) -> R {
        if i == 0 {
            f(&self.primary.font)
        } else {
            f(&self.fallbacks.borrow()[i - 1].font)
        }
    }

    pub fn ascent(&self) -> f32 {
        let s = self.scale_for(&self.primary.font);
        self.primary.font.as_scaled(s).ascent()
    }

    /// Height of a line, like GDI's tmHeight (ascent plus descent).
    pub fn line_height(&self) -> i32 {
        let s = self.scale_for(&self.primary.font);
        let sf = self.primary.font.as_scaled(s);
        (sf.ascent() - sf.descent()).ceil() as i32
    }

    /// The pen position after each character of `s`, starting at 0.
    pub fn advances(&self, s: &[char]) -> Vec<f32> {
        let mut out = Vec::with_capacity(s.len());
        let mut x = 0.0f32;
        let mut prev: Option<(usize, ab_glyph::GlyphId)> = None;
        for &c in s {
            let i = self.pick(c);
            let (id, adv, kern) = self.with(i, |f| {
                let sf = f.as_scaled(self.scale_for(f));
                let id = f.glyph_id(c);
                let kern = match prev {
                    Some((pi, pid)) if pi == i => sf.kern(pid, id),
                    _ => 0.0,
                };
                (id, sf.h_advance(id), kern)
            });
            x += kern + adv;
            out.push(x);
            prev = Some((i, id));
        }
        out
    }

    /// Draws `s` with its baseline at (x, y), calling `put(x, y, coverage)`.
    pub fn draw(&self, s: &[char], x: f32, y: f32, put: &mut dyn FnMut(i32, i32, f32)) {
        let mut pen = x;
        let mut prev: Option<(usize, ab_glyph::GlyphId)> = None;
        for &c in s {
            let i = self.pick(c);
            let (id, adv) = self.with(i, |f| {
                let scale = self.scale_for(f);
                let sf = f.as_scaled(scale);
                let id = f.glyph_id(c);
                if let Some((pi, pid)) = prev {
                    if pi == i {
                        pen += sf.kern(pid, id);
                    }
                }
                let g: Glyph = id.with_scale_and_position(scale, ab_glyph::point(pen, y));
                if let Some(o) = f.outline_glyph(g) {
                    let b = o.px_bounds();
                    o.draw(|gx, gy, cov| put(b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, cov));
                }
                (id, sf.h_advance(id))
            });
            pen += adv;
            prev = Some((i, id));
        }
    }
}
