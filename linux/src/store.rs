//! Disk layout on Linux, the same idea as on Windows. Data that should follow
//! the user lives in the Documents folder, so backups and sync carry it along.
//!
//!   <Documents>/Crow FO/crow.ini        settings and per-PC positions
//!   <Documents>/Crow FO/f<id>/*         the items of folder <id>
//!
//! An item is a launcher (.desktop file, the Linux shortcut) or a symbolic
//! link to a file or folder. Data that can be rebuilt stays local:
//!
//!   ~/.cache/crowfo/f<id>.icons         memory-mapped icon cache

use crate::ffi::*;
use crate::util::*;
use alloc::vec::Vec;
use std::path::{Path, PathBuf};

static mut ROOT: Option<WString> = None;

fn s(w: &[u16]) -> String {
    p(w).to_string_lossy().into_owned()
}
/// A path from the shared code, which joins with `\\`.
fn p(w: &[u16]) -> PathBuf {
    crate::ffi::lin_path(w)
}
fn w(p: &Path) -> WString {
    to_w(&p.to_string_lossy())
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// A user folder from ~/.config/user-dirs.dirs (XDG_DOCUMENTS_DIR...), which
/// follows the user's language, like ~/Documentos.
pub fn user_dir(key: &str, fallback: &str) -> PathBuf {
    let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    if let Ok(t) = std::fs::read_to_string(cfg.join("user-dirs.dirs")) {
        for line in t.lines() {
            let Some(v) = line.trim().strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')) else { continue };
            let v = v.trim().trim_matches('"');
            let v = match v.strip_prefix("$HOME") {
                Some(r) => home().join(r.trim_start_matches('/')),
                None => PathBuf::from(v),
            };
            if v.is_absolute() && v != home() && v.is_dir() {
                return v;
            }
        }
    }
    home().join(fallback)
}

/// <Documents>/Crow FO, resolved once per run.
pub fn root() -> WString {
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(ROOT);
        if let Some(r) = slot {
            return r.clone();
        }
        // CROW_DATA overrides the location (portable use, testing)
        let r = match std::env::var("CROW_DATA") {
            Ok(v) if !v.is_empty() => PathBuf::from(v),
            _ => user_dir("XDG_DOCUMENTS_DIR", "Documents").join("Crow FO"),
        };
        let _ = std::fs::create_dir_all(&r);
        let r = w(&r);
        *slot = Some(r.clone());
        r
    }
}

/// ~/.cache/crowfo, local to this machine and never synced.
pub fn cache_dir() -> WString {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache"));
    let d = base.join("crowfo");
    let _ = std::fs::create_dir_all(&d);
    w(&d)
}

pub fn computer_name() -> alloc::string::String {
    let mut buf = [0u16; 64];
    let mut n = 64u32;
    unsafe { GetComputerNameW(buf.as_mut_ptr(), &mut n) };
    from_w(&buf[..(n as usize).min(64)])
}

pub fn ini_path() -> WString {
    wcat(&[&root(), w!("/crow.ini")])
}

pub fn folder_dir(id: u32) -> WString {
    let d = p(&root()).join(format!("f{id}"));
    let _ = std::fs::create_dir(&d);
    w(&d)
}

pub fn desktop_dir() -> WString {
    w(&user_dir("XDG_DESKTOP_DIR", "Desktop"))
}

/// Builds `dir/name`, adding " (2)", " (3)" and so on before the extension when the name is taken.
pub fn unique_path(dir: &[u16], name: &[u16]) -> WString {
    let first = wcat(&[dir, w!("/"), name]);
    if !taken(&first) {
        return first;
    }
    let st = stem(name);
    let ext = &name[st.len()..];
    for i in 2..1000 {
        let p = wcat(&[dir, w!("/"), st, &to_w(&format!(" ({i})")), ext]);
        if !taken(&p) {
            return p;
        }
    }
    first
}

/// Exists, even as a broken link.
fn taken(path: &[u16]) -> bool {
    std::fs::symlink_metadata(p(path)).is_ok()
}

pub fn is_shortcut(path: &[u16]) -> bool {
    ext_is(path, "desktop")
}

pub enum Import {
    Done(WString),
    /// Kept for the shared code. Linux has no shared desktop to unlock.
    NeedsAdmin,
    Failed,
}

/// Whether dropping `p` on a folder moves it in: anything that sits on the
/// desktop does, like dropping it on a desktop folder would. Everything else
/// stays where it is: the folder gets a copy of a launcher (so apps stay in
/// the menu) or a link to a file.
pub fn moves_in(path: &[u16]) -> bool {
    let path = trim_nul(path);
    let parent = parent_dir(path);
    !parent.is_empty() && p(parent) == p(&desktop_dir())
}

/// Brings `src` into folder `dir` and returns the new file name.
pub fn import(dir: &[u16], src: &[u16]) -> Import {
    let src = trim_nul(src);
    if p(parent_dir(src)) == p(dir) {
        return Import::Done(file_name(src).to_vec());
    }
    if !taken(src) {
        return Import::Failed;
    }
    if moves_in(src) {
        let dst = unique_path(dir, &item_name(src));
        let meta = icon_meta(&p(src));
        if std::fs::rename(p(src), p(&dst)).is_ok() {
            set_icon_meta(&p(&dst), &meta);
            return Import::Done(file_name(&dst).to_vec());
        }
        // another drive: keep the original and keep it reachable
    }
    match fallback_in(dir, src) {
        Some(n) => Import::Done(n),
        None => Import::Failed,
    }
}

/// The name an item gets in a folder. Launchers are named after the app, so
/// the label reads "Firefox" and not "firefox_firefox".
fn item_name(src: &[u16]) -> WString {
    if is_shortcut(src) {
        if let Some(e) = DesktopEntry::read(&p(src)) {
            let n = clean_name(&e.name);
            if !n.is_empty() {
                return to_w(&format!("{n}.desktop"));
            }
        }
    }
    file_name(src).to_vec()
}

fn clean_name(n: &str) -> String {
    n.chars().map(|c| if c == '/' || c == '\0' { '-' } else { c }).collect::<String>().trim().trim_start_matches('.').to_string()
}

/// For an item that isn't moved in: a launcher is copied, anything else gets a link to it.
pub fn fallback_in(dir: &[u16], src: &[u16]) -> Option<WString> {
    if is_shortcut(src) { copy_in(dir, src) } else { link_in(dir, src) }
}

fn link_in(dir: &[u16], src: &[u16]) -> Option<WString> {
    let dst = unique_path(dir, file_name(src));
    if !unsafe { create_link(&dst, src) } {
        return None;
    }
    set_icon_meta(&p(&dst), &icon_meta(&p(src)));
    Some(file_name(&dst).to_vec())
}

fn copy_in(dir: &[u16], src: &[u16]) -> Option<WString> {
    let dst = unique_path(dir, &item_name(src));
    std::fs::copy(p(src), p(&dst)).ok()?;
    trust(&p(&dst));
    Some(file_name(&dst).to_vec())
}

/// Launchers need to be executable (and trusted by Nemo and Nautilus) to start
/// from the desktop.
pub fn trust(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = std::fs::metadata(path) {
        let mut perm = m.permissions();
        perm.set_mode(perm.mode() | 0o755 & 0o111 | 0o600);
        let _ = std::fs::set_permissions(path, perm);
    }
    if has_program("gio") {
        let _ = std::process::Command::new("gio")
            .args(["set", &path.to_string_lossy(), "metadata::trusted", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// The label a folder shows for item `name`: launchers without ".desktop",
/// everything else in full, like Linux file managers show them.
pub fn label<'a>(_dir: &[u16], name: &'a [u16]) -> &'a [u16] {
    let name = trim_nul(name);
    if is_shortcut(name) { stem(name) } else { name }
}

/// Links `lnk` to `target` (a symbolic link).
pub unsafe fn create_link(lnk: &[u16], target: &[u16]) -> bool {
    std::os::unix::fs::symlink(p(target), p(lnk)).is_ok()
}

/// Moves an item out of the organizer back to the desktop.
pub fn export_to_desktop(path: &[u16]) -> bool {
    let desk = desktop_dir();
    let _ = std::fs::create_dir_all(p(&desk));
    let dst = unique_path(&desk, file_name(path));
    let meta = icon_meta(&p(path));
    if move_path(&p(path), &p(&dst)) {
        set_icon_meta(&p(&dst), &meta);
        if is_shortcut(&dst) {
            trust(&p(&dst));
        }
        return true;
    }
    false
}

/// rename, or copy and delete across file systems.
fn move_path(a: &Path, b: &Path) -> bool {
    if std::fs::rename(a, b).is_ok() {
        return true;
    }
    let Ok(m) = std::fs::symlink_metadata(a) else { return false };
    if m.file_type().is_symlink() {
        let Ok(t) = std::fs::read_link(a) else { return false };
        return std::os::unix::fs::symlink(t, b).is_ok() && std::fs::remove_file(a).is_ok();
    }
    if m.is_file() {
        return std::fs::copy(a, b).is_ok() && std::fs::remove_file(a).is_ok();
    }
    false
}

pub fn move_between(src: &[u16], dir: &[u16]) -> Option<WString> {
    let dst = unique_path(dir, file_name(src));
    let meta = icon_meta(&p(src));
    if !move_path(&p(src), &p(&dst)) {
        return None;
    }
    set_icon_meta(&p(&dst), &meta);
    Some(file_name(&dst).to_vec())
}

/// Renames item `old` of `dir` to what the user typed over its label. A
/// launcher keeps its extension, and its name inside changes too so the
/// desktop and menus show the same.
pub fn rename(dir: &[u16], old: &[u16], new_label: &[u16]) -> Option<WString> {
    let ext = &old[label(dir, old).len()..];
    let clean: WString = new_label.iter().copied().filter(|&c| c != b'/' as u16 && c != 0).collect();
    let clean = to_w(from_w(&clean).trim());
    if clean.is_empty() {
        return None;
    }
    let dst = unique_path(dir, &wcat(&[&clean, ext]));
    let src = wcat(&[dir, w!("/"), old]);
    std::fs::rename(p(&src), p(&dst)).ok()?;
    if is_shortcut(&dst) {
        DesktopEntry::set_name(&p(&dst), &from_w(&clean));
    }
    Some(file_name(&dst).to_vec())
}

// ------------------------------------------------------------------ launching
pub fn launch(path: &[u16]) {
    let path = p(path);
    if path.extension().is_some_and(|e| e == "desktop") {
        if let Some(e) = DesktopEntry::read(&path) {
            e.launch(&path);
            return;
        }
    }
    // Programs (an AppImage, a script) run, everything else opens.
    let target = std::fs::canonicalize(&path).unwrap_or(path.clone());
    if is_program(&target) {
        spawn(&target.to_string_lossy(), &[], target.parent());
        return;
    }
    open_default(&path.to_string_lossy());
}

fn is_program(p: &Path) -> bool {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    let Ok(m) = std::fs::metadata(p) else { return false };
    if !m.is_file() || m.permissions().mode() & 0o111 == 0 {
        return false;
    }
    let mut head = [0u8; 4];
    let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    (n == 4 && &head == b"\x7fELF") || (n >= 2 && &head[..2] == b"#!")
}

/// Shows the item in the file manager, selected when it can be.
pub fn reveal(path: &[u16]) {
    let path = p(path);
    let uri = file_uri(&path);
    if has_program("dbus-send") {
        let ok = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return;
        }
    }
    if let Some(d) = path.parent() {
        open_default(&d.to_string_lossy());
    }
}

pub fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

// ------------------------------------------------------------- desktop entries
/// The parts of a .desktop file Crow needs.
#[derive(Default)]
pub struct DesktopEntry {
    pub name: String,
    pub icon: String,
    pub exec: String,
    pub path: String,
    pub url: String,
    pub kind: String,
    pub terminal: bool,
}

impl DesktopEntry {
    pub fn read(path: &Path) -> Option<DesktopEntry> {
        let t = std::fs::read(path).ok()?;
        if t.len() > 256 << 10 {
            return None;
        }
        Some(Self::parse(&String::from_utf8_lossy(&t)))
    }

    /// The [Desktop Entry] group, with the name in the user's language when there is one.
    pub fn parse(t: &str) -> DesktopEntry {
        let mut e = DesktopEntry::default();
        let langs = locale_keys();
        let mut best = usize::MAX;
        let mut in_main = false;
        for line in t.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                in_main = l == "[Desktop Entry]";
                continue;
            }
            if !in_main || l.starts_with('#') {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let (k, v) = (k.trim(), unescape(v.trim()));
            match k {
                "Name" if best == usize::MAX => e.name = v,
                "Icon" => e.icon = v,
                "Exec" => e.exec = v,
                "Path" => e.path = v,
                "URL" => e.url = v,
                "Type" => e.kind = v,
                "Terminal" => e.terminal = v == "true",
                _ => {
                    if let Some(loc) = k.strip_prefix("Name[").and_then(|r| r.strip_suffix(']')) {
                        if let Some(rank) = langs.iter().position(|x| x == loc) {
                            if rank < best {
                                best = rank;
                                e.name = v;
                            }
                        }
                    }
                }
            }
        }
        e
    }

    /// Starts the app, or opens the link, of a launcher.
    pub fn launch(&self, file: &Path) {
        if self.kind == "Link" {
            if !self.url.is_empty() {
                open_default(&self.url);
            }
            return;
        }
        let mut args = split_exec(&self.exec, self, file);
        if args.is_empty() {
            return;
        }
        if self.terminal {
            let term = ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "mate-terminal", "xterm"]
                .into_iter()
                .find(|t| has_program(t));
            if let Some(t) = term {
                let mut v = vec![t.to_string(), if t == "gnome-terminal" { "--".into() } else { "-e".into() }];
                v.append(&mut args);
                args = v;
            }
        }
        let dir = (!self.path.is_empty()).then(|| PathBuf::from(&self.path)).or_else(|| Some(home()));
        let rest: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();
        spawn(&args[0], &rest, dir.as_deref());
    }

    /// Changes the name shown for a launcher, in every language.
    pub fn set_name(path: &Path, name: &str) {
        let Ok(t) = std::fs::read_to_string(path) else { return };
        let mut out = String::with_capacity(t.len());
        let mut in_main = false;
        let mut done = false;
        for line in t.lines() {
            let l = line.trim_start();
            if l.starts_with('[') {
                in_main = l.trim() == "[Desktop Entry]";
            } else if in_main && (l.starts_with("Name[") || l.starts_with("Name=") || l.starts_with("Name ")) {
                if !done && (l.starts_with("Name=") || l.starts_with("Name ")) {
                    out.push_str("Name=");
                    out.push_str(name);
                    out.push('\n');
                    done = true;
                }
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        let _ = std::fs::write(path, out);
    }
}

/// Name[...] keys to look for, best first: pt_BR, pt, then nothing.
fn locale_keys() -> Vec<String> {
    let v = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty()).unwrap_or_default();
    let base = v.split(['.', '@']).next().unwrap_or("").to_string();
    let mut out = vec![base.clone()];
    if let Some((l, _)) = base.split_once('_') {
        out.push(l.to_string());
    }
    out.retain(|s| !s.is_empty() && s != "C" && s != "POSIX");
    out
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an Exec line into arguments and fills in its field codes, as the
/// Desktop Entry specification describes.
fn split_exec(exec: &str, e: &DesktopEntry, file: &Path) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut quoted = false;
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                have = true;
            }
            '\\' if quoted => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if have || !cur.is_empty() {
                    args.push(core::mem::take(&mut cur));
                    have = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if have || !cur.is_empty() {
        args.push(cur);
    }
    let mut out = Vec::new();
    for a in args {
        match a.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
            "%i" => {
                if !e.icon.is_empty() {
                    out.push("--icon".to_string());
                    out.push(e.icon.clone());
                }
                continue;
            }
            _ => {}
        }
        let mut s = String::new();
        let mut it = a.chars();
        while let Some(c) = it.next() {
            if c != '%' {
                s.push(c);
                continue;
            }
            match it.next() {
                Some('%') => s.push('%'),
                Some('c') => s.push_str(&e.name),
                Some('k') => s.push_str(&file.to_string_lossy()),
                _ => {}
            }
        }
        out.push(s);
    }
    out
}

// --------------------------------------------------------------- autostart
pub fn own_exe() -> WString {
    w(&std::env::current_exe().unwrap_or_default())
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

pub fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"))
}

/// Quotes a path for an Exec line.
pub fn exec_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.+".contains(c)) {
        return s.to_string();
    }
    let mut o = String::from("\"");
    for c in s.chars() {
        if "\"`$\\".contains(c) {
            o.push('\\');
        }
        o.push(c);
    }
    o.push('"');
    o.replace('\\', "\\\\")
}

pub fn autostart_file() -> PathBuf {
    config_home().join("autostart/crowfo.desktop")
}

/// Turns "start at login" on or off for this executable.
pub fn set_autostart(on: bool) {
    set_autostart_for(&own_exe(), on);
}

/// Same, for a given executable (used by the installer). It starts with
/// --background so the settings don't open at every login.
pub fn set_autostart_for(exe: &[u16], on: bool) {
    let f = autostart_file();
    if !on {
        let _ = std::fs::remove_file(&f);
        return;
    }
    let body = format!(
        "[Desktop Entry]\nType=Application\nName=Crow FO\nExec={} --background\nIcon=crowfo\nX-GNOME-Autostart-enabled=true\nNoDisplay=false\nTerminal=false\n",
        exec_quote(&s(exe))
    );
    if std::fs::read_to_string(&f).is_ok_and(|t| t == body) {
        return;
    }
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(&f, body);
}

// ------------------------------------------------- desktop context menu
// The desktop belongs to the file manager on Linux. Crow adds its entries to
// the ones it can: Nemo (Cinnamon, also the desktop there), Nautilus and
// Caja scripts, and Dolphin/Plasma service menus.
fn menu_files() -> Vec<PathBuf> {
    let d = data_home();
    vec![
        d.join("nemo/actions/crowfo-new.nemo_action"),
        d.join("nemo/actions/crowfo-panel.nemo_action"),
        d.join("nemo/actions/crowfo-settings.nemo_action"),
        d.join("kio/servicemenus/crowfo.desktop"),
        d.join("kservices5/ServiceMenus/crowfo.desktop"),
        home().join(".local/share/nautilus/scripts/Crow FO"),
        config_home().join("caja/scripts/Crow FO"),
    ]
}

pub fn set_desktop_menu(on: bool) {
    set_desktop_menu_for(&own_exe(), on);
}

pub fn set_desktop_menu_for(exe: &[u16], on: bool) {
    for f in menu_files() {
        if f.is_dir() {
            let _ = std::fs::remove_dir_all(&f);
        } else {
            let _ = std::fs::remove_file(&f);
        }
    }
    if !on {
        return;
    }
    crate::installer::install_icons();
    let exe = exec_quote(&s(exe));
    let tr = crate::i18n::tr;
    let d = data_home();
    let nemo = |file: &str, name: &str, arg: &str, icon: &str, order: &str| {
        let body = format!(
            "[Nemo Action]\nName=Crow FO: {name}\nComment=Crow FO\nExec={exe} {arg}\nIcon-Name={icon}\nSelection=None\nExtensions=any;\nConditions=desktop;\nEscapeSpaces=true\n# {order}\n"
        );
        let p = d.join("nemo/actions").join(file);
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, body);
    };
    nemo("crowfo-new.nemo_action", tr("menu.new_folder"), "--new", "crowfo-new", "1");
    nemo("crowfo-panel.nemo_action", tr("menu.new_panel"), "--new-panel", "crowfo-panel", "2");
    nemo("crowfo-settings.nemo_action", tr("menu.app_settings"), "--settings", "crowfo-settings", "3");

    // Plasma: a submenu on the desktop and in folders
    let kde = format!(
        "[Desktop Entry]\nType=Service\nMimeType=inode/directory;\nX-KDE-ServiceTypes=KonqPopupMenu/Plugin\nX-KDE-Submenu=Crow FO\nActions=new;panel;settings\n\n\
         [Desktop Action new]\nName={}\nIcon=crowfo-new\nExec={exe} --new\n\n\
         [Desktop Action panel]\nName={}\nIcon=crowfo-panel\nExec={exe} --new-panel\n\n\
         [Desktop Action settings]\nName={}\nIcon=crowfo-settings\nExec={exe} --settings\n",
        tr("menu.new_folder"),
        tr("menu.new_panel"),
        tr("menu.app_settings")
    );
    for f in [d.join("kio/servicemenus/crowfo.desktop"), d.join("kservices5/ServiceMenus/crowfo.desktop")] {
        if f.parent().and_then(|p| p.parent()).is_some_and(|p| p.is_dir()) {
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(&f, &kde);
            make_exec(&f);
        }
    }

    // Nautilus and Caja: a "Crow FO" scripts submenu
    for dir in [home().join(".local/share/nautilus/scripts/Crow FO"), config_home().join("caja/scripts/Crow FO")] {
        if !dir.parent().is_some_and(|p| p.parent().is_some_and(|q| q.is_dir())) {
            continue;
        }
        let _ = std::fs::create_dir_all(&dir);
        for (name, arg) in [(tr("menu.new_folder"), "--new"), (tr("menu.new_panel"), "--new-panel"), (tr("menu.app_settings"), "--settings")] {
            let f = dir.join(clean_name(name));
            let _ = std::fs::write(&f, format!("#!/bin/sh\nexec {exe} {arg}\n"));
            make_exec(&f);
        }
    }
}

fn make_exec(f: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755));
}

/// Moves a file to the trash.
pub fn recycle(path: &[u16]) -> bool {
    let path = p(path);
    if has_program("gio") {
        let ok = std::process::Command::new("gio")
            .args(["trash", "--", &path.to_string_lossy()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return std::fs::symlink_metadata(&path).is_err();
        }
    }
    // The freedesktop.org trash: the file under files/, its origin under info/.
    let trash = data_home().join("Trash");
    let (files, info) = (trash.join("files"), trash.join("info"));
    let _ = std::fs::create_dir_all(&files);
    let _ = std::fs::create_dir_all(&info);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut n = name.clone();
    let mut i = 2;
    while files.join(&n).exists() || info.join(format!("{n}.trashinfo")).exists() {
        n = format!("{name}.{i}");
        i += 1;
    }
    let date = std::process::Command::new("date").arg("+%Y-%m-%dT%H:%M:%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let uri = file_uri(&path);
    let enc = uri.trim_start_matches("file://");
    if std::fs::write(info.join(format!("{n}.trashinfo")), format!("[Trash Info]\nPath={enc}\nDeletionDate={date}\n")).is_err() {
        return false;
    }
    if move_path(&path, &files.join(&n)) {
        return true;
    }
    let _ = std::fs::remove_file(info.join(format!("{n}.trashinfo")));
    false
}

// -------------------------------------------------------------- icon cache
/// Read-only memory-mapped icon cache for one folder. Same format as on Windows.
pub struct IconCache {
    map: Option<memmap2::Mmap>,
}

const MAGIC: u32 = 0x3157_5243; // "CRW1"

impl IconCache {
    pub const fn empty() -> IconCache {
        IconCache { map: None }
    }
    pub fn path(dir: &[u16]) -> WString {
        wcat(&[&cache_dir(), w!("/"), file_name(dir), w!(".icons")])
    }
    pub fn open(dir: &[u16]) -> IconCache {
        let Ok(f) = std::fs::File::open(p(&Self::path(dir))) else { return Self::empty() };
        if f.metadata().map(|m| m.len()).unwrap_or(0) < 8 {
            return Self::empty();
        }
        let Ok(m) = (unsafe { memmap2::Mmap::map(&f) }) else { return Self::empty() };
        let c = IconCache { map: Some(m) };
        if c.u32_at(0) != MAGIC {
            return Self::empty();
        }
        c
    }
    pub fn is_open(&self) -> bool {
        self.map.is_some()
    }
    fn u32_at(&self, off: usize) -> u32 {
        match &self.map {
            Some(m) if off + 4 <= m.len() => u32::from_le_bytes([m[off], m[off + 1], m[off + 2], m[off + 3]]),
            _ => 0,
        }
    }
    /// Finds an icon of s by s pixels by its key.
    pub fn get(&self, key: u64, s: i32) -> Option<&[u32]> {
        let m = self.map.as_ref()?;
        let n = self.u32_at(4) as usize;
        for i in 0..n {
            let e = 8 + i * 16;
            let k = self.u32_at(e) as u64 | (self.u32_at(e + 4) as u64) << 32;
            if k == key && self.u32_at(e + 8) as i32 == s {
                let off = self.u32_at(e + 12) as usize;
                let cnt = (s * s) as usize;
                if off % 4 != 0 || off + cnt * 4 > m.len() {
                    return None;
                }
                return Some(unsafe { core::slice::from_raw_parts(m.as_ptr().add(off) as *const u32, cnt) });
            }
        }
        None
    }
    pub fn close(&mut self) {
        self.map = None;
    }
}

/// Writes a new cache file with the given (key, size, pixels) entries.
pub fn write_cache(dir: &[u16], entries: &[(u64, i32, Vec<u32>)]) {
    let n = entries.len();
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    let mut off = 8 + n * 16;
    for (k, s, px) in entries {
        out.extend_from_slice(&k.to_le_bytes());
        out.extend_from_slice(&(*s as u32).to_le_bytes());
        out.extend_from_slice(&(off as u32).to_le_bytes());
        off += px.len() * 4;
    }
    for (_, _, px) in entries {
        for v in px {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    // Replace atomically: the old file may still be mapped.
    let path = p(&IconCache::path(dir));
    let tmp = path.with_extension("icons.tmp");
    if std::fs::write(&tmp, &out).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Whether the file manager menu entries are in place.
pub fn desktop_menu_present() -> bool {
    menu_files().iter().any(|f| f.exists())
}

// ------------------------------------------------------------ custom icons
/// The icon a user picked for a file in the file manager (Nemo, Nautilus,
/// Caja). GIO keeps it as metadata of the path, so it doesn't follow a plain
/// rename: Crow reads it before moving a file and sets it again after.
const ICON_KEYS: [&str; 2] = ["metadata::custom-icon", "metadata::custom-icon-name"];

pub fn icon_meta(path: &Path) -> Vec<(String, String)> {
    if !has_program("gio") {
        return Vec::new();
    }
    let Ok(o) = std::process::Command::new("gio")
        .args(["info", "--nofollow-symlinks", "-a", &ICON_KEYS.join(","), "--", &path.to_string_lossy()])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in String::from_utf8_lossy(&o.stdout).lines() {
        let l = l.trim();
        for k in ICON_KEYS {
            if let Some(v) = l.strip_prefix(k).and_then(|r| r.strip_prefix(": ")) {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}

fn set_icon_meta(path: &Path, meta: &[(String, String)]) {
    for (k, v) in meta {
        let _ = std::process::Command::new("gio")
            .args(["set", "--nofollow-symlinks", "--", &path.to_string_lossy(), k, v])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// A local path from a file:// URI.
pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}
