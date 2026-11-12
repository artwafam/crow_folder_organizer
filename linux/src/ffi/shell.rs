//! Clipboard, opening files and links, the file picker, the UI language and
//! starting other programs.

use super::x::x;
use super::*;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;

// ---------------------------------------------------------------- processes
/// Starts a program detached from Crow, so it outlives it and never waits on it.
pub fn spawn(cmd: &str, args: &[&str], dir: Option<&std::path::Path>) -> bool {
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new(cmd);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(d) = dir.filter(|d| d.is_dir()) {
        c.current_dir(d);
    }
    unsafe {
        c.pre_exec(|| {
            // a new session, and a second fork so the child is adopted by init
            libc::setsid();
            match libc::fork() {
                -1 => Err(std::io::Error::last_os_error()),
                0 => Ok(()),
                _ => libc::_exit(0),
            }
        });
    }
    match c.spawn() {
        Ok(mut ch) => {
            let _ = ch.wait(); // the intermediate process exits right away
            true
        }
        Err(_) => false,
    }
}

pub fn has_program(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// Opens a file, folder or link with its default application.
pub fn open_default(target: &str) -> bool {
    for (cmd, pre) in [("xdg-open", None), ("gio", Some("open"))] {
        if has_program(cmd) {
            let args: Vec<&str> = pre.into_iter().chain([target]).collect();
            if spawn(cmd, &args, None) {
                return true;
            }
        }
    }
    false
}

pub unsafe extern "system" fn ShellExecuteW(_h: HWND, _op: PCWSTR, file: PCWSTR, params: PCWSTR, dir: PCWSTR, _show: i32) -> isize {
    let f = wstring(file);
    let p = wstring(params);
    let ok = if f.eq_ignore_ascii_case("notepad.exe") {
        // "Edit crow.ini" opens it in the text editor
        let path = lin_path(&p.encode_utf16().collect::<Vec<u16>>());
        open_text(&path.to_string_lossy())
    } else if f.starts_with("http://") || f.starts_with("https://") || f.starts_with("mailto:") {
        open_default(&f)
    } else {
        let d = lin_pathp(dir);
        let path = lin_path(&f.encode_utf16().collect::<Vec<u16>>());
        if p.is_empty() {
            open_default(&path.to_string_lossy())
        } else {
            spawn(&path.to_string_lossy(), &p.split_whitespace().collect::<Vec<_>>(), Some(&d))
        }
    };
    if ok { 33 } else { 2 }
}

/// Opens a text file in the user's text editor, whatever its extension.
fn open_text(path: &str) -> bool {
    if let Ok(o) = std::process::Command::new("xdg-mime").args(["query", "default", "text/plain"]).output() {
        let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !id.is_empty() && has_program("gtk-launch") && spawn("gtk-launch", &[&id, path], None) {
            return true;
        }
    }
    for ed in ["xed", "gnome-text-editor", "gedit", "kate", "kwrite", "mousepad", "pluma", "featherpad"] {
        if has_program(ed) {
            return spawn(ed, &[path], None);
        }
    }
    open_default(path)
}

// ----------------------------------------------------------------- dialogs
/// The open file dialog, through zenity or kdialog.
pub unsafe extern "system" fn GetOpenFileNameW(o: *mut OPENFILENAMEW) -> BOOL {
    let o = &mut *o;
    let title = wstring(o.lpstrTitle);
    // the filter is pairs of NUL-terminated strings, ending with an empty one
    let mut parts: Vec<String> = Vec::new();
    let mut p = o.lpstrFilter;
    if !p.is_null() {
        loop {
            let s = wstring(p);
            if s.is_empty() {
                break;
            }
            p = p.add(s.encode_utf16().count() + 1);
            parts.push(s);
        }
    }
    let filters: Vec<(String, Vec<String>)> = parts
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].clone(), c[1].split(';').map(|g| g.trim().to_string()).filter(|g| g != "*.*").collect()))
        .collect();
    let start = std::env::var("HOME").unwrap_or_default();
    let out = if has_program("zenity") {
        let mut args: Vec<String> = vec!["--file-selection".into(), format!("--title={title}"), format!("--filename={start}/")];
        for (name, globs) in &filters {
            let g = if globs.is_empty() { "*".to_string() } else { globs.join(" ") };
            args.push(format!("--file-filter={} | {}", name.replace('|', "/"), g));
        }
        std::process::Command::new("zenity").args(&args).output().ok()
    } else if has_program("kdialog") {
        let f: Vec<String> = filters.iter().map(|(n, g)| format!("{} ({})", g.join(" "), n)).collect();
        std::process::Command::new("kdialog").args(["--title", &title, "--getopenfilename", &start, &f.join("\n")]).output().ok()
    } else {
        None
    };
    let Some(out) = out.filter(|o| o.status.success()) else { return 0 };
    let path = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    if path.is_empty() {
        return 0;
    }
    let w: Vec<u16> = path.encode_utf16().collect();
    if w.len() + 1 > o.nMaxFile as usize {
        return 0;
    }
    put_w(&w, o.lpstrFile, o.nMaxFile);
    1
}

pub unsafe extern "system" fn MessageBoxW(_h: HWND, t: PCWSTR, c: PCWSTR, f: u32) -> i32 {
    let (text, cap) = (wstring(t), wstring(c));
    let yesno = f & 0xF == MB_YESNO;
    let kind = if yesno { "--question" } else if f & 0xF0 == MB_ICONWARNING { "--warning" } else { "--info" };
    if has_program("zenity") {
        let ok = std::process::Command::new("zenity").args([kind, &format!("--title={cap}"), &format!("--text={text}"), "--no-markup"]).status().is_ok_and(|s| s.success());
        return if yesno { if ok { IDYES } else { 7 } } else { 1 };
    }
    if has_program("kdialog") {
        let k = if yesno { "--yesno" } else if kind == "--warning" { "--sorry" } else { "--msgbox" };
        let ok = std::process::Command::new("kdialog").args(["--title", &cap, k, &text]).status().is_ok_and(|s| s.success());
        return if yesno { if ok { IDYES } else { 7 } } else { 1 };
    }
    eprintln!("{cap}: {text}");
    if yesno { 7 } else { 1 }
}

// ----------------------------------------------------------------- language
/// The Windows language id that matches the session locale, which is what
/// the shared code expects to read.
pub unsafe extern "system" fn GetUserDefaultUILanguage() -> u16 {
    let v = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_default();
    // LANGUAGE can list several, like "pt_BR:pt:en"
    let first = v.split(':').next().unwrap_or("").split(['.', '@']).next().unwrap_or("").to_ascii_lowercase();
    let (lang, region) = first.split_once(['_', '-']).unwrap_or((&first, ""));
    match lang {
        "pt" => 0x0416,
        "es" => 0x0C0A,
        "ru" => 0x0419,
        "zh" => match region {
            "tw" | "hk" | "mo" | "hant" => 0x0404,
            _ => 0x0804,
        },
        _ => 0x0409,
    }
}

// ---------------------------------------------------------------- clipboard
static mut CLIP: Option<Vec<u16>> = None;
static mut OPEN: bool = false;
static mut STAGED: Option<Vec<u16>> = None;

pub unsafe extern "system" fn OpenClipboard(_h: HWND) -> BOOL {
    OPEN = true;
    1
}

pub unsafe extern "system" fn CloseClipboard() -> BOOL {
    OPEN = false;
    1
}

pub unsafe extern "system" fn EmptyClipboard() -> BOOL {
    STAGED = None;
    1
}

pub unsafe extern "system" fn SetClipboardData(f: u32, h: isize) -> isize {
    if f != CF_UNICODETEXT {
        return 0;
    }
    let Some(bytes) = kernel::global_take(h) else { return 0 };
    let w: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    STAGED = Some(w.clone());
    CLIP = Some(w);
    let xs = x();
    let owner = win::utility_window();
    let _ = xs.conn.set_selection_owner(owner, xs.atoms.CLIPBOARD, x11rb::CURRENT_TIME);
    xs.flush();
    h
}

pub unsafe extern "system" fn GetClipboardData(f: u32) -> isize {
    if f != CF_UNICODETEXT {
        return 0;
    }
    let xs = x();
    let owner = xs.conn.get_selection_owner(xs.atoms.CLIPBOARD).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.owner);
    let text: Option<Vec<u16>> = if owner != 0 && owner == win::utility_window() {
        (*core::ptr::addr_of!(CLIP)).clone()
    } else if owner != 0 {
        read_selection(xs.atoms.CLIPBOARD).map(|s| s.encode_utf16().collect())
    } else {
        None
    };
    let Some(mut w) = text else { return 0 };
    w.push(0);
    let bytes: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    kernel::global_put(bytes)
}

/// Reads a selection as UTF-8 text, waiting briefly for the owner to answer.
pub fn read_selection(sel: Atom) -> Option<String> {
    let xs = x();
    let me = win::utility_window();
    let prop = xs.atoms.CROW_SELECTION;
    let _ = xs.conn.convert_selection(me, sel, xs.atoms.UTF8_STRING, prop, x11rb::CURRENT_TIME);
    let ev = xs.wait_for(1000, &|e| matches!(e, Event::SelectionNotify(n) if n.requestor == me && n.selection == sel))?;
    let Event::SelectionNotify(n) = ev else { return None };
    if n.property == x11rb::NONE {
        return None;
    }
    let data = xs.prop_bytes(me, prop, AtomEnum::ANY);
    let _ = xs.conn.delete_property(me, prop);
    Some(String::from_utf8_lossy(&data).into_owned())
}

/// Answers other programs asking for what Crow copied.
pub fn clipboard_event(e: &Event) -> bool {
    let xs = x();
    let a = &xs.atoms;
    match e {
        Event::SelectionClear(c) if c.selection == a.CLIPBOARD => {
            unsafe { CLIP = None };
            true
        }
        Event::SelectionRequest(r) if r.selection == a.CLIPBOARD && r.owner == win::utility_window() => {
            let text = unsafe { (*core::ptr::addr_of!(CLIP)).clone() }.map(|w| String::from_utf16_lossy(&w)).unwrap_or_default();
            let prop = if r.property == x11rb::NONE { r.target } else { r.property };
            let ok = if r.target == a.TARGETS {
                let _ = xs.conn.change_property32(PropMode::REPLACE, r.requestor, prop, AtomEnum::ATOM, &[a.TARGETS, a.UTF8_STRING, a.STRING, a.TEXT]);
                true
            } else if r.target == a.UTF8_STRING || r.target == a.TEXT || r.target == u32::from(AtomEnum::STRING) {
                let _ = xs.conn.change_property8(PropMode::REPLACE, r.requestor, prop, r.target, text.as_bytes());
                true
            } else {
                false
            };
            let ev = SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: r.time,
                requestor: r.requestor,
                selection: r.selection,
                target: r.target,
                property: if ok { prop } else { x11rb::NONE },
            };
            let _ = xs.conn.send_event(false, r.requestor, EventMask::NO_EVENT, ev);
            xs.flush();
            true
        }
        _ => false,
    }
}
