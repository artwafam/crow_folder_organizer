//! Icons the way Linux desktops find them: the user's icon theme (and the
//! themes it inherits), file types from shared-mime-info, and launcher icons.
//! See the freedesktop.org Icon Theme and Shared MIME-info specifications.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct Dir {
    path: String,
    size: i32,
    min: i32,
    max: i32,
    scale: i32,
    scalable: bool,
}

struct Theme {
    bases: Vec<PathBuf>,
    dirs: Vec<Dir>,
    inherits: Vec<String>,
}

struct Icons {
    themes: HashMap<String, Option<Theme>>,
    chain: Vec<String>,
    mime_ext: Option<(HashMap<String, String>, Vec<(String, String)>)>,
    generic: Option<HashMap<String, String>>,
}

static mut I: Option<Icons> = None;

fn st() -> &'static mut Icons {
    unsafe {
        (*core::ptr::addr_of_mut!(I)).get_or_insert_with(|| Icons { themes: HashMap::new(), chain: Vec::new(), mime_ext: None, generic: None })
    }
}

/// Forgets the theme and file type tables until the next icon is needed.
pub fn release() {
    unsafe { *core::ptr::addr_of_mut!(I) = None };
}

fn data_dirs() -> Vec<PathBuf> {
    let mut v = vec![crate::store::data_home()];
    let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    v.extend(dirs.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    v
}

fn icon_bases() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = std::env::var_os("HOME") {
        v.push(PathBuf::from(h).join(".icons"));
    }
    v.extend(data_dirs().into_iter().map(|d| d.join("icons")));
    v
}

/// The icon theme of the desktop: XSETTINGS (GTK desktops), then KDE's setting.
fn current_theme() -> String {
    if let Some(t) = xsettings_string("Net/IconThemeName") {
        return t;
    }
    if let Some(h) = std::env::var_os("HOME") {
        if let Ok(t) = std::fs::read_to_string(PathBuf::from(h).join(".config/kdeglobals")) {
            let mut in_icons = false;
            for l in t.lines() {
                let l = l.trim();
                if l.starts_with('[') {
                    in_icons = l == "[Icons]";
                } else if in_icons {
                    if let Some(v) = l.strip_prefix("Theme=") {
                        return v.trim().to_string();
                    }
                }
            }
        }
    }
    for k in ["org.cinnamon.desktop.interface", "org.gnome.desktop.interface", "org.mate.interface"] {
        if let Ok(o) = std::process::Command::new("gsettings").args(["get", k, "icon-theme"]).stderr(std::process::Stdio::null()).output() {
            let v = String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string();
            if o.status.success() && !v.is_empty() {
                return v;
            }
        }
    }
    "hicolor".into()
}

/// A string setting from the XSETTINGS manager (the desktop's settings daemon).
pub fn xsettings_string(name: &str) -> Option<String> {
    use x11rb::protocol::xproto::*;
    let xs = crate::ffi::x::x();
    let sel = xs.atom(&format!("_XSETTINGS_S{}", xs.screen));
    let owner = xs.conn.get_selection_owner(sel).ok()?.reply().ok()?.owner;
    if owner == 0 {
        return None;
    }
    let prop = xs.atom("_XSETTINGS_SETTINGS");
    let d = xs.prop_bytes(owner, prop, AtomEnum::ANY);
    if d.len() < 12 {
        return None;
    }
    let big = d[0] == 1;
    let u16_at = |o: usize| -> usize {
        let b = [d[o], d[o + 1]];
        (if big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) }) as usize
    };
    let u32_at = |o: usize| -> usize {
        let b = [d[o], d[o + 1], d[o + 2], d[o + 3]];
        (if big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }) as usize
    };
    let pad = |n: usize| (n + 3) & !3;
    let n = u32_at(8);
    let mut o = 12;
    for _ in 0..n {
        if o + 4 > d.len() {
            break;
        }
        let ty = d[o];
        let nl = u16_at(o + 2);
        let key = d.get(o + 4..o + 4 + nl)?;
        o += 4 + pad(nl) + 4; // name, then last-change serial
        match ty {
            0 => o += 4,
            1 => {
                let vl = u32_at(o);
                let v = d.get(o + 4..o + 4 + vl)?;
                if key == name.as_bytes() {
                    return Some(String::from_utf8_lossy(v).into_owned());
                }
                o += 4 + pad(vl);
            }
            2 => o += 8,
            _ => break,
        }
    }
    let _ = &xs.conn;
    None
}

fn load_theme(name: &str) -> Option<Theme> {
    let bases: Vec<PathBuf> = icon_bases().into_iter().map(|b| b.join(name)).filter(|p| p.is_dir()).collect();
    let index = bases.iter().find_map(|b| std::fs::read_to_string(b.join("index.theme")).ok())?;
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut cur = String::new();
    for l in index.lines() {
        let l = l.trim();
        if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            cur = s.to_string();
        } else if let Some((k, v)) = l.split_once('=') {
            sections.entry(cur.clone()).or_default().insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let main = sections.get("Icon Theme")?;
    let mut list: Vec<String> = Vec::new();
    for k in ["Directories", "ScaledDirectories"] {
        if let Some(v) = main.get(k) {
            list.extend(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
        }
    }
    let inherits = main.get("Inherits").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let mut dirs = Vec::new();
    for d in list {
        let Some(s) = sections.get(&d) else { continue };
        let num = |k: &str| s.get(k).and_then(|v| v.parse::<i32>().ok());
        let size = num("Size").unwrap_or(48);
        let ty = s.get("Type").map(|v| v.as_str()).unwrap_or("Threshold");
        let th = num("Threshold").unwrap_or(2);
        let (min, max) = match ty {
            "Fixed" => (size, size),
            "Scalable" => (num("MinSize").unwrap_or(size), num("MaxSize").unwrap_or(size)),
            _ => (size - th, size + th),
        };
        // symbolic icons are meant to be recolored, they look wrong as app icons
        if d.contains("symbolic") {
            continue;
        }
        dirs.push(Dir { path: d, size, min, max, scale: num("Scale").unwrap_or(1).max(1), scalable: ty == "Scalable" });
    }
    Some(Theme { bases, dirs, inherits })
}

fn chain() -> &'static Vec<String> {
    let s = st();
    if s.chain.is_empty() {
        let mut out: Vec<String> = Vec::new();
        let mut todo = vec![current_theme()];
        while let Some(t) = todo.pop() {
            if out.contains(&t) || out.len() > 12 {
                continue;
            }
            let th = s.themes.entry(t.clone()).or_insert_with(|| load_theme(&t));
            if let Some(th) = th {
                for i in th.inherits.iter().rev() {
                    todo.push(i.clone());
                }
            }
            out.push(t);
        }
        if !out.iter().any(|t| t == "hicolor") {
            s.themes.entry("hicolor".into()).or_insert_with(|| load_theme("hicolor"));
            out.push("hicolor".into());
        }
        s.chain = out;
    }
    &s.chain
}

/// Candidate files for icon `name`, best first.
fn find(name: &str, size: i32) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for t in chain().clone() {
        let Some(Some(th)) = st().themes.get(&t) else { continue };
        // distance from the wanted size; a bigger image scales down better
        let mut found: Vec<(i32, PathBuf)> = Vec::new();
        for d in &th.dirs {
            for b in &th.bases {
                for ext in ["png", "svg", "xpm"] {
                    if ext == "xpm" {
                        continue;
                    }
                    let f = b.join(&d.path).join(format!("{name}.{ext}"));
                    if !f.is_file() {
                        continue;
                    }
                    let px = d.size * d.scale;
                    let score = if ext == "svg" && (d.scalable || d.min <= size && size <= d.max) {
                        1
                    } else if px == size {
                        0
                    } else if px > size {
                        10 + (px - size)
                    } else {
                        1000 + (size - px)
                    };
                    found.push((score, f));
                }
            }
        }
        if !found.is_empty() {
            found.sort_by_key(|f| f.0);
            out.extend(found.into_iter().map(|f| f.1));
            return out;
        }
    }
    // icons outside themes
    for d in data_dirs() {
        for ext in ["png", "svg"] {
            let f = d.join("pixmaps").join(format!("{name}.{ext}"));
            if f.is_file() {
                out.push(f);
            }
        }
    }
    out
}

/// A themed icon as premultiplied s by s pixels.
pub fn theme_icon(name: &str, s: i32) -> Option<Vec<u32>> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return crate::image::decode_path(Path::new(name), s);
    }
    let name = name.strip_suffix(".png").or_else(|| name.strip_suffix(".svg")).unwrap_or(name);
    find(name, s).into_iter().find_map(|f| crate::image::decode_path(&f, s))
}

// ------------------------------------------------------------- file types
fn mime_tables() -> &'static (HashMap<String, String>, Vec<(String, String)>) {
    let s = st();
    s.mime_ext.get_or_insert_with(|| {
        let mut ext: HashMap<String, (i32, String)> = HashMap::new();
        let mut pats: Vec<(String, String)> = Vec::new();
        for d in data_dirs().iter().rev() {
            let Ok(t) = std::fs::read_to_string(d.join("mime/globs2")) else { continue };
            for l in t.lines() {
                if l.starts_with('#') {
                    continue;
                }
                let mut it = l.splitn(4, ':');
                let (Some(w), Some(m), Some(g)) = (it.next(), it.next(), it.next()) else { continue };
                let w: i32 = w.parse().unwrap_or(50);
                if let Some(e) = g.strip_prefix("*.") {
                    if !e.contains(['*', '?', '[']) {
                        let e = e.to_lowercase();
                        if ext.get(&e).map_or(true, |x| x.0 < w) {
                            ext.insert(e, (w, m.to_string()));
                        }
                        continue;
                    }
                }
                pats.push((g.to_string(), m.to_string()));
            }
        }
        (ext.into_iter().map(|(k, v)| (k, v.1)).collect(), pats)
    })
}

fn generic_icons() -> &'static HashMap<String, String> {
    st().generic.get_or_insert_with(|| {
        let mut m = HashMap::new();
        for d in data_dirs().iter().rev() {
            for f in ["mime/generic-icons", "mime/icons"] {
                if let Ok(t) = std::fs::read_to_string(d.join(f)) {
                    for l in t.lines() {
                        if let Some((a, b)) = l.split_once(':') {
                            m.insert(a.to_string(), b.to_string());
                        }
                    }
                }
            }
        }
        m
    })
}

/// The MIME type of a file name, from its extension (longest match first).
pub fn mime_of(name: &str) -> Option<String> {
    let (ext, pats) = mime_tables();
    let lower = name.to_lowercase();
    let mut i = 0;
    while let Some(p) = lower[i..].find('.') {
        let e = &lower[i + p + 1..];
        if let Some(m) = ext.get(e) {
            return Some(m.clone());
        }
        i += p + 1;
    }
    pats.iter().find(|(g, _)| g == name).map(|(_, m)| m.clone())
}

fn special_folder(p: &Path) -> &'static str {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let is = |k: &str, f: &str| crate::store::user_dir(k, f) == p;
    if p == home {
        "user-home"
    } else if is("XDG_DESKTOP_DIR", "Desktop") {
        "user-desktop"
    } else if is("XDG_DOCUMENTS_DIR", "Documents") {
        "folder-documents"
    } else if is("XDG_DOWNLOAD_DIR", "Downloads") {
        "folder-download"
    } else if is("XDG_MUSIC_DIR", "Music") {
        "folder-music"
    } else if is("XDG_PICTURES_DIR", "Pictures") {
        "folder-pictures"
    } else if is("XDG_VIDEOS_DIR", "Videos") {
        "folder-videos"
    } else {
        "folder"
    }
}

/// The icon a file manager would show for `path`.
pub fn for_path(path: &Path, s: i32) -> Option<Vec<u32>> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // an icon the user picked in the file manager wins
    for (k, v) in crate::store::icon_meta(path) {
        let px = if k.ends_with("custom-icon-name") {
            theme_icon(&v, s)
        } else {
            crate::store::uri_path(&v).and_then(|f| crate::image::decode_path(&f, s))
        };
        if px.is_some() {
            return px;
        }
    }
    if name.ends_with(".desktop") {
        if let Some(e) = crate::store::DesktopEntry::read(path) {
            if let Some(px) = theme_icon(&e.icon, s) {
                return Some(px);
            }
            let fallback = if e.kind == "Link" { "text-html" } else { "application-x-executable" };
            return theme_icon(fallback, s);
        }
    }
    let target = std::fs::canonicalize(path).ok();
    let Some(target) = target else {
        // a link whose target is gone
        return theme_icon("emblem-unreadable", s).or_else(|| theme_icon("text-x-generic", s));
    };
    if target.is_dir() {
        let n = special_folder(&target);
        return theme_icon(n, s).or_else(|| theme_icon("folder", s));
    }
    let tname = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mime = mime_of(&tname).or_else(|| mime_of(&name));
    // Pictures show themselves, like thumbnails.
    if mime.as_deref().is_some_and(|m| m.starts_with("image/")) && std::fs::metadata(&target).is_ok_and(|m| m.len() < 24 << 20) {
        if let Some(px) = crate::image::decode_path(&target, s) {
            return Some(px);
        }
    }
    if tname.ends_with(".AppImage") || tname.ends_with(".appimage") {
        if let Some(px) = theme_icon("application-x-executable", s) {
            return Some(px);
        }
    }
    if let Some(m) = mime {
        if let Some(px) = theme_icon(&m.replace('/', "-"), s) {
            return Some(px);
        }
        if let Some(g) = generic_icons().get(&m) {
            if let Some(px) = theme_icon(g, s) {
                return Some(px);
            }
        }
        let top = m.split('/').next().unwrap_or("");
        if let Some(px) = theme_icon(&format!("{top}-x-generic"), s) {
            return Some(px);
        }
    }
    use std::os::unix::fs::PermissionsExt;
    if std::fs::metadata(&target).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
        // Programs carry no icon on Linux. The theme may have one named after
        // the app, like "godot" for Godot_v4.7-stable_linux.x86_64.
        let word: String = tname.split(['_', '-', '.', ' ']).next().unwrap_or("").to_lowercase();
        if word.len() >= 3 {
            if let Some(px) = theme_icon(&word, s) {
                return Some(px);
            }
        }
        if let Some(px) = theme_icon("application-x-executable", s) {
            return Some(px);
        }
    }
    theme_icon("text-x-generic", s).or_else(|| theme_icon("unknown", s))
}

/// Prints each step of finding the icon of `path`.
pub fn debug(path: &Path, s: i32) {
    println!("theme chain: {:?}", chain());
    if let Some(e) = crate::store::DesktopEntry::read(path) {
        println!("launcher icon: {:?}", e.icon);
        for f in find(&e.icon, s) {
            println!("  candidate {} -> {}", f.display(), crate::image::decode_path(&f, s).map_or("failed".into(), |p| format!("{} px", p.len())));
        }
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mime = mime_of(&name);
    println!("mime: {mime:?}");
    if let Some(m) = &mime {
        for n in [m.replace('/', "-"), generic_icons().get(m).cloned().unwrap_or_default(), "text-x-generic".into()] {
            for f in find(&n, s).into_iter().take(3) {
                println!("  {n}: {} -> {}", f.display(), crate::image::decode_path(&f, s).map_or("failed".into(), |p| format!("{} px", p.len())));
            }
        }
    }
    println!("result: {}", for_path(path, s).map_or("none".into(), |p| format!("{} px", p.len())));
}
