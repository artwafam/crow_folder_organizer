//! Update check, the same flow as on Windows. When the user allows it, each
//! start of the installed app runs `crowfo --check-update`, a separate process
//! that asks GitHub for the latest release and quits right away when there is
//! nothing new. The "Check for updates" button runs the same with `--now` and
//! reports back to the app.
//!
//! When a newer version exists, a small window offers it. "Update" downloads
//! the Linux binary of the release, checks it against the release's
//! SHA256SUMS.txt and runs it with `--install --quiet`, which replaces the
//! installed copy and starts it again. Settings and folders stay as they are.

use crate::ffi::*;
use crate::store;
use std::path::PathBuf;

/// The release file with the Linux build.
pub const ASSET: &str = "CrowFO-linux-x86_64";

/// Per computer settings that don't belong in crow.ini, which may be synced.
fn settings_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    base.join("crowfo/local.ini")
}

/// Whether the user allowed the check at startup.
pub fn enabled() -> bool {
    choice() == Some(true)
}

/// The user's answer, or None if they were never asked.
pub fn choice() -> Option<bool> {
    let t = std::fs::read_to_string(settings_file()).ok()?;
    t.lines().find_map(|l| l.trim().strip_prefix("check_updates=")).map(|v| v.trim() == "1")
}

pub fn set_enabled(on: bool) {
    let f = settings_file();
    let mut lines: Vec<String> = std::fs::read_to_string(&f).unwrap_or_default().lines().filter(|l| !l.trim().starts_with("check_updates=")).map(String::from).collect();
    lines.push(format!("check_updates={}", u8::from(on)));
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(&f, lines.join("\n") + "\n");
}

/// Updates replace the installed copy, so a copy run from elsewhere doesn't check.
pub fn is_installed() -> bool {
    let me = std::env::current_exe().ok().and_then(|p| std::fs::canonicalize(p).ok());
    let inst = std::fs::canonicalize(crate::installer::exe_path()).ok();
    me.is_some() && me == inst
}

/// What the settings show about the "Check for updates" button. The check
/// process sends the last three back.
pub const CHECKING: u8 = 1;
pub const LATEST: u8 = 2;
pub const FAILED: u8 = 3;
pub const FOUND: u8 = 4;

fn own() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Starts a check right away, for the button in the settings.
pub fn check_now() -> bool {
    spawn(&own(), &["--check-update", "--now"], None)
}

/// Tells the running app how a check from the button ended.
pub fn report(state: u8) {
    crate::instance::send(&format!("update {state}"));
}

/// Called once each time the app starts.
pub fn on_start() {
    if !is_installed() || !enabled() {
        return;
    }
    spawn(&own(), &["--check-update"], None);
}

/// "owner/name" of the GitHub repository from Cargo.toml.
fn repo() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/").trim_end_matches('/')
}

pub fn releases_page() -> String {
    format!("https://github.com/{}/releases/latest", repo())
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
    let k = format!("\"{key}\"");
    let rest = text[text.find(&k)? + k.len()..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].into())
}

pub enum Check {
    /// A newer release, by its tag.
    Newer(String),
    Latest,
    Failed,
}

/// Asks GitHub for the latest release. Right after login the network may
/// still be coming up, so with `patient` a failed connection is tried again a
/// few times. Any real answer ends it.
pub fn check(patient: bool) -> Check {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
    let waits: &[u64] = if patient { &[0, 15, 30, 60, 120] } else { &[0] };
    for &wait in waits {
        std::thread::sleep(std::time::Duration::from_secs(wait));
        match crate::net::get(&url, "application/vnd.github+json", 1 << 20) {
            Ok(body) => {
                let Some(text) = core::str::from_utf8(&body).ok() else { return Check::Failed };
                let Some(tag) = json_str(text, "tag_name") else { return Check::Failed };
                // the tag goes into a URL, so only plain version characters
                let plain = tag.len() < 40 && tag.bytes().all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c));
                if !plain {
                    return Check::Failed;
                }
                // a release without a Linux build is not an update for Linux
                if !text.contains(ASSET) {
                    return Check::Latest;
                }
                return if is_newer(&tag, env!("CARGO_PKG_VERSION")) { Check::Newer(tag) } else { Check::Latest };
            }
            Err(crate::net::Error::Refused) => return Check::Failed,
            Err(crate::net::Error::Network) => {}
        }
    }
    Check::Failed
}

fn updates_dir() -> PathBuf {
    PathBuf::from(crate::util::from_w(&store::cache_dir())).join("updates")
}

/// Removes the download left by the last update.
pub fn cleanup() {
    let _ = std::fs::remove_file(updates_dir().join(ASSET));
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Downloads the Linux build of release `tag`, checks it and installs it.
/// Returns false if anything goes wrong, and nothing is run in that case.
pub fn download_and_run(tag: &str) -> bool {
    let base = format!("https://github.com/{}/releases/download/{tag}/", repo());
    let Ok(sums) = crate::net::get(&format!("{base}SHA256SUMS.txt"), "*/*", 64 * 1024) else { return false };
    let Ok(sums) = core::str::from_utf8(&sums) else { return false };
    let Some(want) = sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, name) = (it.next()?, it.next()?);
        (name.trim_start_matches('*') == ASSET && h.len() == 64).then_some(h)
    }) else {
        return false;
    };
    let Ok(bin) = crate::net::get(&format!("{base}{ASSET}"), "application/octet-stream", 32 << 20) else { return false };
    let Some(got) = crate::net::sha256(&bin) else { return false };
    if !hex(&got).eq_ignore_ascii_case(want) || !bin.starts_with(b"\x7fELF") {
        return false;
    }
    let dir = updates_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(ASSET);
    if std::fs::write(&path, &bin).is_err() {
        return false;
    }
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    spawn(&path.to_string_lossy(), &["--install", "--quiet"], None)
}

/// The language picked in Crow FO settings, for the update window.
pub fn language() -> String {
    crate::util::read_file(&store::ini_path())
        .map(|d| crate::config::Config::parse(core::str::from_utf8(&d).unwrap_or("")).g.language)
        .unwrap_or_else(|| "auto".into())
}

/// The folder of the per computer settings, removed on uninstall.
pub fn settings_dir() -> Option<PathBuf> {
    settings_file().parent().map(|p| p.to_path_buf())
}
