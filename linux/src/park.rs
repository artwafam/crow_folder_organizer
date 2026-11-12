//! Quitting with the "Quit Crow FO" button puts every item back on the
//! desktop, so nothing stays hidden in a folder while Crow isn't running.
//! The next start moves them into their folders again. Logging out, updates
//! and uninstalling don't do this; their items stay in the folders.
//!
//! Where each item went is kept per computer (the desktop is local, while the
//! data folder may be synced):
//!
//!   ~/.config/crowfo/parked     folder id, item name, desktop path (tab separated)

use crate::store;
use crate::util::*;
use std::path::{Path, PathBuf};

fn record() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    base.join("crowfo/parked")
}

/// The item folders inside the data folder, as (id, path).
fn folders() -> Vec<(u32, PathBuf)> {
    let root = crate::ffi::lin_path(&store::root());
    let mut v: Vec<(u32, PathBuf)> = std::fs::read_dir(&root)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let id = n.strip_prefix('f')?.parse().ok()?;
                    e.path().is_dir().then(|| (id, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn w(p: &Path) -> WString {
    to_w(&p.to_string_lossy())
}

/// Moves every item of every folder to the desktop and remembers it.
pub fn park() {
    let desk = store::desktop_dir();
    let _ = std::fs::create_dir_all(crate::ffi::lin_path(&desk));
    let mut lines = std::fs::read_to_string(record()).unwrap_or_default();
    for (id, dir) in folders() {
        for name in list_files(&w(&dir)) {
            let src = dir.join(from_w(&name));
            let Some(new) = store::move_between(&w(&src), &desk) else { continue };
            let dst = crate::ffi::lin_path(&desk).join(from_w(&new));
            if store::is_shortcut(&new) {
                store::trust(&dst);
            }
            lines.push_str(&format!("{id}\t{}\t{}\n", from_w(&name), dst.to_string_lossy()));
        }
    }
    let f = record();
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(&f, lines);
}

/// Brings parked items back into their folders. Items that were renamed,
/// moved or deleted in the meantime stay where the user left them.
pub fn unpark() {
    let f = record();
    let Ok(t) = std::fs::read_to_string(&f) else { return };
    let root = crate::ffi::lin_path(&store::root());
    for line in t.lines() {
        let mut it = line.splitn(3, '\t');
        let (Some(id), Some(name), Some(at)) = (it.next(), it.next(), it.next()) else { continue };
        let at = Path::new(at);
        let dir = root.join(format!("f{id}"));
        if std::fs::symlink_metadata(at).is_err() || !dir.is_dir() {
            continue;
        }
        let _ = store::move_between(&w(at), &w(&dir)).map(|got| {
            // it gets its old name back when that is free (the folder order uses it)
            let target = dir.join(name);
            let now = dir.join(from_w(&got));
            if now != target && std::fs::symlink_metadata(&target).is_err() {
                let _ = std::fs::rename(&now, &target);
            }
        });
    }
    let _ = std::fs::remove_file(&f);
}
