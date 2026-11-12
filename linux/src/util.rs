//! ../src/util.rs as is, with the few helpers that mean something else on Linux.

#[macro_use]
#[path = "../../src/util.rs"]
mod base;
pub use base::*;

/// A plain absolute path on this machine. Paths written on Windows (C:\...)
/// and network-looking ones (//host/...) are refused, like the Windows build
/// refuses UNC paths from a synced crow.ini.
pub fn is_local_path(p: &[u16]) -> bool {
    let p = trim_nul(p);
    p.len() >= 2 && p[0] == b'/' as u16 && p[1] != b'/' as u16 && p[1] != b'\\' as u16
}

/// The change time of a file, as on Windows, plus the time GIO metadata last
/// changed. Icons picked in the file manager live there, so folders read
/// their icons again after the user picks a new one.
pub fn mtime(path: &[u16]) -> u64 {
    let m = base::mtime(path);
    if m == 0 || is_dir(path) {
        return m;
    }
    m ^ metadata_epoch().rotate_left(17)
}

fn metadata_epoch() -> u64 {
    static mut LAST: (f64, u64) = (-1e9, 0);
    unsafe {
        let now = now_ms();
        let last = &mut *core::ptr::addr_of_mut!(LAST);
        if now - last.0 < 1000.0 {
            return last.1;
        }
        // The journal is written through a memory map, which doesn't always
        // update its time, so its contents tell when something changed.
        let dir = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".local/share/gvfs-metadata");
        let mut newest = 0u64;
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                // each tree's database ("home", "root"...) and its journal, not old copies
                if !n.contains('.') || n.ends_with(".log") {
                    if let Ok(d) = std::fs::read(e.path()) {
                        newest = fnv(newest, &d);
                    }
                }
            }
        }
        *last = (now, newest);
        newest
    }
}
