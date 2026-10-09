//! Settings model and a small INI reader and writer (UTF-8, easy to edit by hand).

use crate::svg::parse_color;
use crate::util::*;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hover,
    Panel,
}

/// How the panel header is set apart from the content.
pub const HEADER_NAMES: [&str; 4] = ["tint", "band", "line", "none"];
pub fn header_parse(v: &str) -> Option<u8> {
    HEADER_NAMES.iter().position(|n| v.eq_ignore_ascii_case(n)).map(|i| i as u8)
}

#[derive(Clone)]
pub struct Global {
    pub font: String,
    pub font_size: i32,
    pub title_size: i32,
    pub icon_size: i32,
    pub cell_w: i32,
    pub hover_delay: u32,
    pub close_delay: u32,
    pub anim_ms: u32,
    pub blur: bool,
    pub autostart: bool,
    pub bg: u32,
    pub border: u32,
    pub text: u32,
    pub title: u32,
    pub hover: u32,
    pub radius: i32,
    pub shadow: bool,
    pub label_shadow: bool,
    pub snap: i32,
    pub on_top: bool,
    pub blur_kind: i32,
    pub header: u8,
    pub header_strength: i32,
    pub show_count: bool,
    pub desktop_menu: bool,
    /// Language code, or "auto" to follow Windows.
    pub language: String,
    /// Defaults for new folders. A color of 0 picks a different one each time.
    pub def_color: u32,
    pub def_size: i32,
    pub def_columns: i32,
}

impl Default for Global {
    fn default() -> Self {
        Global {
            font: "Segoe UI Variable Text".into(),
            font_size: 12,
            title_size: 13,
            icon_size: 44,
            cell_w: 88,
            hover_delay: 40,
            close_delay: 80,
            anim_ms: 200,
            blur: false,
            autostart: true,
            bg: 0xFF1D1E23,
            border: 0x30FFFFFF,
            text: 0xFFF2F3F7,
            title: 0xFFFFFFFF,
            hover: 0x24FFFFFF,
            radius: 16,
            shadow: true,
            label_shadow: true,
            snap: 8,
            on_top: false,
            blur_kind: 0,
            header: 0,
            header_strength: 24,
            show_count: false,
            desktop_menu: true,
            language: "auto".into(),
            def_color: 0,
            def_size: 64,
            def_columns: 5,
        }
    }
}

#[derive(Clone)]
pub struct Folder {
    pub id: u32,
    pub name: String,
    pub mode: Mode,
    pub x: i32,
    pub y: i32,
    pub columns: i32,
    pub rows: i32,
    pub size: i32,
    pub color: u32,
    pub icon: String,
    pub tint: u32,
    pub label: bool,
    pub locked: bool,
    /// Panels can be rolled up to just their header.
    pub collapsed: bool,
    pub icon_size: i32,
    pub bg: u32, // 0 = use global
    pub header: Option<u8>,
    /// Positions saved per computer as (computer name, x, y)
    pub pos: Vec<(String, i32, i32)>,
    pub order: Vec<String>,
    pub item_icons: Vec<(String, String)>,
}

impl Folder {
    pub fn new(id: u32, name: &str, x: i32, y: i32) -> Folder {
        Folder {
            id,
            name: name.into(),
            mode: Mode::Hover,
            x,
            y,
            columns: 5,
            rows: 4,
            size: 64,
            color: 0xFFF5B841,
            icon: String::new(),
            tint: 0xFFFFFFFF,
            label: true,
            locked: false,
            collapsed: false,
            icon_size: 0,
            bg: 0,
            header: None,
            pos: Vec::new(),
            order: Vec::new(),
            item_icons: Vec::new(),
        }
    }
    pub fn item_icon(&self, file: &str) -> Option<&str> {
        self.item_icons.iter().find(|(f, _)| f.eq_ignore_ascii_case(file)).map(|(_, p)| p.as_str())
    }
}

pub struct Config {
    pub g: Global,
    pub folders: Vec<Folder>,
}

fn color_str(c: u32, out: &mut String) {
    const H: &[u8] = b"0123456789ABCDEF";
    out.push('#');
    let a = c >> 24;
    let mut push = |v: u32| {
        out.push(H[(v >> 4) as usize] as char);
        out.push(H[(v & 15) as usize] as char);
    };
    push((c >> 16) & 255);
    push((c >> 8) & 255);
    push(c & 255);
    if a != 255 {
        push(a);
    }
}

fn b(v: &str) -> bool {
    matches!(v.trim(), "1" | "true" | "yes" | "on")
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut g = Global::default();
        let mut folders: Vec<Folder> = Vec::new();
        let mut in_folder = false;
        let mut version = 0;
        for raw in text.lines() {
            let line = raw.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                let sec = line.trim_matches(|c| c == '[' || c == ']').trim();
                in_folder = sec.eq_ignore_ascii_case("folder");
                if in_folder {
                    let id = folders.iter().map(|f| f.id).max().unwrap_or(0) + 1;
                    folders.push(Folder::new(id, "Folder", 120, 120));
                }
                continue;
            }
            let Some(eq) = line.find('=') else { continue };
            let k = line[..eq].trim().to_ascii_lowercase();
            let v = line[eq + 1..].trim();
            let int = |d: i32| parse_int(v).map(|x| x as i32).unwrap_or(d);
            let col = |d: u32| parse_color(v.as_bytes()).unwrap_or(d);
            if in_folder {
                let f = folders.last_mut().unwrap();
                match k.as_str() {
                    "id" => f.id = int(f.id as i32).max(1) as u32,
                    "name" => f.name = v.into(),
                    "mode" => f.mode = if v.eq_ignore_ascii_case("panel") { Mode::Panel } else { Mode::Hover },
                    "x" => f.x = int(f.x),
                    "y" => f.y = int(f.y),
                    "columns" => f.columns = int(f.columns).clamp(1, 16),
                    "rows" => f.rows = int(f.rows).clamp(1, 12),
                    "size" => f.size = int(f.size).clamp(24, 256),
                    "color" => f.color = col(f.color),
                    "icon" => f.icon = v.into(),
                    "tint" => f.tint = col(f.tint),
                    "label" => f.label = b(v),
                    "locked" => f.locked = b(v),
                    "collapsed" => f.collapsed = b(v),
                    "icon_size" => f.icon_size = int(0).clamp(0, 256),
                    "bg" => f.bg = col(0),
                    "header" => f.header = header_parse(v),
                    "pos" => {
                        let mut it = v.rsplitn(3, ',');
                        if let (Some(py), Some(px), Some(n)) = (it.next(), it.next(), it.next()) {
                            if let (Some(px), Some(py)) = (parse_int(px), parse_int(py)) {
                                f.pos.push((n.trim().into(), px as i32, py as i32));
                            }
                        }
                    }
                    "items" => f.order = v.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).map(String::from).collect(),
                    "item_icon" => {
                        if let Some((a, p)) = v.split_once('|') {
                            f.item_icons.push((a.trim().into(), p.trim().into()));
                        }
                    }
                    _ => {}
                }
            } else {
                match k.as_str() {
                    "font" => g.font = v.into(),
                    "font_size" => g.font_size = int(g.font_size).clamp(6, 48),
                    "title_size" => g.title_size = int(g.title_size).clamp(6, 64),
                    "icon_size" => g.icon_size = int(g.icon_size).clamp(16, 256),
                    "cell_width" => g.cell_w = int(g.cell_w).clamp(40, 400),
                    "hover_delay" => g.hover_delay = int(g.hover_delay as i32).clamp(0, 5000) as u32,
                    "close_delay" => g.close_delay = int(g.close_delay as i32).clamp(0, 10000) as u32,
                    "animation_ms" => g.anim_ms = int(g.anim_ms as i32).clamp(0, 3000) as u32,
                    "blur" => g.blur = b(v),
                    "autostart" => g.autostart = b(v),
                    "version" => version = int(0),
                    "background" => g.bg = col(g.bg),
                    "border" => g.border = col(g.border),
                    "text" => g.text = col(g.text),
                    "title" => g.title = col(g.title),
                    "hover" => g.hover = col(g.hover),
                    "radius" => g.radius = int(g.radius).clamp(0, 64),
                    "shadow" => g.shadow = b(v),
                    "label_shadow" => g.label_shadow = b(v),
                    "snap" => g.snap = int(g.snap).clamp(0, 128),
                    "always_on_top" => g.on_top = b(v),
                    "blur_kind" => g.blur_kind = int(0).clamp(0, 1),
                    "header" => g.header = header_parse(v).unwrap_or(g.header),
                    "header_strength" => g.header_strength = int(g.header_strength).clamp(0, 100),
                    "show_count" => g.show_count = b(v),
                    "desktop_menu" => g.desktop_menu = b(v),
                    "language" => g.language = v.into(),
                    "default_color" => g.def_color = if v.eq_ignore_ascii_case("auto") { 0 } else { col(0) },
                    "default_folder_size" => g.def_size = int(g.def_size).clamp(24, 256),
                    "default_columns" => g.def_columns = int(g.def_columns).clamp(1, 16),
                    _ => {}
                }
            }
        }
        // Files from older versions get the new look: a solid background and
        // the quicker, smoother animation timings.
        let d = Global::default();
        if version < 2 {
            g.bg = d.bg;
        }
        if version < 3 {
            g.anim_ms = d.anim_ms;
            g.hover_delay = d.hover_delay;
            g.close_delay = d.close_delay;
        }
        // Folder ids must be unique
        for i in 0..folders.len() {
            while folders[..i].iter().any(|f| f.id == folders[i].id) {
                folders[i].id += 1;
            }
        }
        Config { g, folders }
    }

    pub fn serialize(&self) -> String {
        let g = &self.g;
        let mut s = String::with_capacity(2048);
        s.push_str("; Crow settings. Edit and save, changes show up right away.\n");
        s.push_str("; Colors are #RRGGBB or #RRGGBBAA, where AA is the opacity.\n; mode is hover (opens when you point at it) or panel (always open).\n");
        s.push_str("; header is tint, band, line or none, here or per folder.\n; The pos lines remember where each folder sits on each computer.\n\n[global]\n");
        let kv = |s: &mut String, k: &str, v: &str| {
            s.push_str(k);
            s.push_str(" = ");
            s.push_str(v);
            s.push('\n');
        };
        let ki = |s: &mut String, k: &str, v: i64| {
            s.push_str(k);
            s.push_str(" = ");
            fmt_int(v, s);
            s.push('\n');
        };
        let kc = |s: &mut String, k: &str, v: u32| {
            s.push_str(k);
            s.push_str(" = ");
            color_str(v, s);
            s.push('\n');
        };
        ki(&mut s, "version", 3);
        kv(&mut s, "language", &g.language);
        kv(&mut s, "font", &g.font);
        ki(&mut s, "font_size", g.font_size as i64);
        ki(&mut s, "title_size", g.title_size as i64);
        ki(&mut s, "icon_size", g.icon_size as i64);
        ki(&mut s, "cell_width", g.cell_w as i64);
        ki(&mut s, "hover_delay", g.hover_delay as i64);
        ki(&mut s, "close_delay", g.close_delay as i64);
        ki(&mut s, "animation_ms", g.anim_ms as i64);
        ki(&mut s, "blur", g.blur as i64);
        ki(&mut s, "autostart", g.autostart as i64);
        kc(&mut s, "background", g.bg);
        kc(&mut s, "border", g.border);
        kc(&mut s, "text", g.text);
        kc(&mut s, "title", g.title);
        kc(&mut s, "hover", g.hover);
        ki(&mut s, "radius", g.radius as i64);
        ki(&mut s, "shadow", g.shadow as i64);
        ki(&mut s, "label_shadow", g.label_shadow as i64);
        ki(&mut s, "snap", g.snap as i64);
        ki(&mut s, "always_on_top", g.on_top as i64);
        kv(&mut s, "header", HEADER_NAMES[g.header as usize & 3]);
        ki(&mut s, "header_strength", g.header_strength as i64);
        ki(&mut s, "show_count", g.show_count as i64);
        ki(&mut s, "desktop_menu", g.desktop_menu as i64);
        if g.def_color == 0 {
            kv(&mut s, "default_color", "auto");
        } else {
            kc(&mut s, "default_color", g.def_color);
        }
        ki(&mut s, "default_folder_size", g.def_size as i64);
        ki(&mut s, "default_columns", g.def_columns as i64);
        for f in &self.folders {
            s.push_str("\n[folder]\n");
            ki(&mut s, "id", f.id as i64);
            kv(&mut s, "name", &f.name);
            kv(&mut s, "mode", if f.mode == Mode::Panel { "panel" } else { "hover" });
            ki(&mut s, "x", f.x as i64);
            ki(&mut s, "y", f.y as i64);
            ki(&mut s, "columns", f.columns as i64);
            ki(&mut s, "rows", f.rows as i64);
            ki(&mut s, "size", f.size as i64);
            kc(&mut s, "color", f.color);
            kv(&mut s, "icon", &f.icon);
            kc(&mut s, "tint", f.tint);
            ki(&mut s, "label", f.label as i64);
            ki(&mut s, "locked", f.locked as i64);
            if f.mode == Mode::Panel {
                ki(&mut s, "collapsed", f.collapsed as i64);
            }
            ki(&mut s, "icon_size", f.icon_size as i64);
            if f.bg != 0 {
                kc(&mut s, "bg", f.bg);
            }
            if let Some(h) = f.header {
                kv(&mut s, "header", HEADER_NAMES[h as usize & 3]);
            }
            for (n, x, y) in &f.pos {
                s.push_str("pos = ");
                s.push_str(n);
                s.push_str(", ");
                fmt_int(*x as i64, &mut s);
                s.push_str(", ");
                fmt_int(*y as i64, &mut s);
                s.push('\n');
            }
            s.push_str("items = ");
            for (i, it) in f.order.iter().enumerate() {
                if i > 0 {
                    s.push_str(" | ");
                }
                s.push_str(it);
            }
            s.push('\n');
            for (a, p) in &f.item_icons {
                s.push_str("item_icon = ");
                s.push_str(a);
                s.push_str(" | ");
                s.push_str(p);
                s.push('\n');
            }
        }
        s
    }
}
