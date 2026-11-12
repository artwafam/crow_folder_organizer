//! Update check. When the user allows it, each start of the installed app runs
//! `CrowFO.exe --check-update`, a separate process that asks GitHub for the
//! latest release and quits right away when there is nothing new. The main app
//! never touches the network, so its memory stays the same.
//!
//! When a newer version exists, a small window offers it. "Update" downloads
//! CrowFOSetup.exe from the release, checks it against the release's
//! SHA256SUMS.txt and runs it with /S. The installer closes Crow FO, replaces
//! it and starts it again, keeping every setting.

use crate::ffi::*;
use crate::store;
use crate::util::*;
use alloc::string::String;
use core::ptr::{null, null_mut};

/// Per computer settings that don't belong in crow.ini, which may be synced.
pub const KEY: &[u16] = w!("Software\\Crow FO");

/// Whether the user allowed the check at startup.
pub fn enabled() -> bool {
    choice() == Some(true)
}

/// The user's answer, or None if they were never asked (an install older
/// than the update check).
pub fn choice() -> Option<bool> {
    unsafe {
        let mut k: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, wz(KEY).as_ptr(), 0, KEY_READ, &mut k) != 0 {
            return None;
        }
        let mut v = 0u32;
        let mut len = 4u32;
        let mut ty = 0u32;
        let r = RegQueryValueExW(k, w!("CheckUpdates").as_ptr(), null_mut(), &mut ty, &mut v as *mut u32 as *mut u8, &mut len);
        RegCloseKey(k);
        (r == 0 && ty == REG_DWORD).then_some(v != 0)
    }
}

pub fn set_enabled(on: bool) {
    store::reg_dword(KEY, w!("CheckUpdates"), on as u32);
}

/// Updates come through the installer, so a portable copy only checks when
/// it is the installed one.
pub fn is_installed() -> bool {
    weq_ic(parent_dir(&store::own_exe()), &crate::installer::install_dir())
}

/// Called once each time the app starts.
pub fn on_start() {
    if !is_installed() || !enabled() {
        return;
    }
    let exe = wz(&store::own_exe());
    unsafe { ShellExecuteW(0, null(), exe.as_ptr(), w!("--check-update").as_ptr(), null(), SW_SHOWNORMAL) };
}

/// "owner/name" of the GitHub repository from Cargo.toml.
fn repo() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/").trim_end_matches('/')
}

/// Joins strings. format! would pull in prebuilt code that needs unwinding.
fn cat(parts: &[&str]) -> String {
    let mut s = String::new();
    for p in parts {
        s.push_str(p);
    }
    s
}

pub fn releases_page() -> String {
    cat(&["https://github.com/", repo(), "/releases/latest"])
}

/// Version numbers and whether it is a pre-release (1.2.0-beta.1).
fn parse(v: &str) -> Option<([u32; 3], bool)> {
    let v = v.trim_start_matches('v');
    let (main, pre) = match v.find('-') {
        Some(i) => (&v[..i], true),
        None => (v.split('+').next().unwrap_or(v), false),
    };
    let mut n = [0u32; 3];
    let mut parts = main.split('.');
    for x in n.iter_mut() {
        *x = parts.next()?.parse().ok()?;
    }
    Some((n, pre))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some((a, ap)), Some((b, bp))) => a > b || (a == b && bp && !ap),
        _ => false,
    }
}

/// The string value of `key` in a JSON text, enough for GitHub's answer.
fn json_str(text: &str, key: &str) -> Option<String> {
    let k = cat(&["\"", key, "\""]);
    let rest = text[text.find(&k)? + k.len()..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].into())
}

/// Asks GitHub for the latest release and returns its tag when it is newer
/// than this build. After sign-in the network may still be coming up, so a
/// failed connection is tried again a few times. Any real answer ends it.
pub fn check() -> Option<String> {
    let url = cat(&["https://api.github.com/repos/", repo(), "/releases/latest"]);
    for wait in [0u32, 15, 30, 60, 120] {
        unsafe { Sleep(wait * 1000) };
        match crate::net::get(&url, "application/vnd.github+json", 1 << 20) {
            Ok(body) => {
                let tag = json_str(core::str::from_utf8(&body).ok()?, "tag_name")?;
                // the tag goes into a URL, so only plain version characters
                let plain = tag.len() < 40 && tag.bytes().all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c));
                return (plain && is_newer(&tag, env!("CARGO_PKG_VERSION"))).then_some(tag);
            }
            Err(crate::net::Error::Refused) => return None,
            Err(crate::net::Error::Network) => {}
        }
    }
    None
}

fn updates_dir() -> WString {
    wcat(&[&env(w!("LOCALAPPDATA")), w!("\\Crow FO\\updates")])
}

/// Removes the installer left by the last update.
pub fn cleanup() {
    unsafe { DeleteFileW(wz(&wcat(&[&updates_dir(), w!("\\CrowFOSetup.exe")])).as_ptr()) };
}

fn hex(b: &[u8]) -> String {
    let mut s = String::new();
    for x in b {
        for d in [x >> 4, x & 15] {
            s.push(char::from_digit(d as u32, 16).unwrap_or('0'));
        }
    }
    s
}

/// Downloads the installer of release `tag`, checks it and starts it. Returns
/// false if anything goes wrong, and nothing is run in that case.
pub fn download_and_run(tag: &str) -> bool {
    let base = cat(&["https://github.com/", repo(), "/releases/download/", tag, "/"]);
    let Ok(sums) = crate::net::get(&cat(&[&base, "SHA256SUMS.txt"]), "*/*", 64 * 1024) else { return false };
    let Ok(sums) = core::str::from_utf8(&sums) else { return false };
    let Some(want) = sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, name) = (it.next()?, it.next()?);
        (name.trim_start_matches('*') == "CrowFOSetup.exe" && h.len() == 64).then_some(h)
    }) else {
        return false;
    };
    let Ok(exe) = crate::net::get(&cat(&[&base, "CrowFOSetup.exe"]), "application/octet-stream", 16 << 20) else { return false };
    let Some(got) = crate::net::sha256(&exe) else { return false };
    if !hex(&got).eq_ignore_ascii_case(want) {
        return false;
    }
    let dir = updates_dir();
    mkdir(&wcat(&[&env(w!("LOCALAPPDATA")), w!("\\Crow FO")]));
    mkdir(&dir);
    let path = wcat(&[&dir, w!("\\CrowFOSetup.exe")]);
    if !write_file(&path, &exe) {
        return false;
    }
    let r = unsafe { ShellExecuteW(0, null(), wz(&path).as_ptr(), w!("/S").as_ptr(), null(), SW_SHOWNORMAL) };
    r > 32
}

/// The language picked in Crow FO settings, for the update window.
pub fn language() -> String {
    read_file(&store::ini_path())
        .map(|d| crate::config::Config::parse(core::str::from_utf8(&d).unwrap_or("")).g.language)
        .unwrap_or_else(|| "auto".into())
}
