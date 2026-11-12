//! Crow FO for Linux (X11).
//!
//! The app itself is the Windows code in ../src, compiled unchanged. It runs
//! on src/ffi, which provides the Win32 functions it uses on top of X11. The
//! modules below that touch the system in ways no Win32 call can stand for
//! (files and shortcuts, icons, drag and drop, stacking, installing, updates)
//! are written for Linux with the same interface as their Windows versions.
#![allow(clippy::missing_safety_doc, clippy::too_many_arguments, static_mut_refs, dead_code)]

extern crate alloc;

#[macro_use]
pub mod util;
pub mod ffi;
pub mod rt;

// ---- shared with Windows, unchanged
#[path = "../../src/actions.rs"]
pub mod actions;
#[path = "../../src/app.rs"]
pub mod app;
#[path = "../../src/art.rs"]
pub mod art;
#[path = "../../src/config.rs"]
pub mod config;
#[path = "../../src/edit.rs"]
pub mod edit;
#[path = "../../src/folder.rs"]
pub mod folder;
#[path = "../../src/gfx.rs"]
pub mod gfx;
#[path = "../../src/glyph.rs"]
pub mod glyph;
#[path = "../../src/i18n.rs"]
pub mod i18n;
#[path = "../../src/input.rs"]
pub mod input;
#[path = "../../src/popup.rs"]
pub mod popup;
#[path = "../../src/prefs.rs"]
pub mod prefs;
#[path = "../../src/raster.rs"]
pub mod raster;
#[path = "../../src/settings.rs"]
pub mod settings;
#[path = "../../src/svg.rs"]
pub mod svg;
#[path = "../../src/text.rs"]
pub mod text;
#[path = "../../src/ui.rs"]
pub mod ui;
#[path = "../../src/uikit.rs"]
pub mod uikit;

// ---- Linux versions
pub mod blur;
pub mod desk;
pub mod drop;
pub mod icons;
pub mod image;
pub mod installer;
pub mod instance;
pub mod net;
pub mod park;
pub mod store;
pub mod update;
pub mod watch;

pub use app::Fonts;

use ffi::*;

fn main() {
    // Pixel buffers come and go with every animation. Given their own
    // mappings they go back to the system as soon as they are freed, instead
    // of staying in the heap, and the heap's top is returned eagerly.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 16 * 1024);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 32 * 1024);
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |s: &str| args.iter().any(|a| a == s);
    let after = |s: &str| args.iter().position(|a| a == s).and_then(|i| args.get(i + 1)).cloned();

    if let Some(dir) = after("--export-art") {
        std::process::exit(if art::export(&util::to_w(&dir)) { 0 } else { 1 });
    }
    if let Some(p) = after("--debug-icon") {
        // Shows how the icon of a file is found (for bug reports).
        icons::debug(std::path::Path::new(&p), 48);
        return;
    }
    if has("--install") {
        std::process::exit(installer::install_cli(has("--quiet") || has("-q")));
    }
    if has("--check-update") {
        // Started by the app at launch, or with --now by the settings button.
        let now = has("--now");
        if let Some(_lock) = instance::lock(if now { "update-now" } else { "update" }) {
            update::cleanup();
            let r = update::check(!now);
            if now {
                update::report(match r {
                    update::Check::Newer(_) => update::FOUND,
                    update::Check::Latest => update::LATEST,
                    update::Check::Failed => update::FAILED,
                });
            }
            if let update::Check::Newer(tag) = r {
                installer::offer_update(tag);
            }
        }
        return;
    }
    if has("--uninstall") {
        installer::uninstall();
        return;
    }
    // The downloaded file run as is: offer to install it first.
    if args.is_empty() && installer::should_offer_install() && !instance::is_running() {
        std::process::exit(installer::install_cli(false));
    }

    let new_req: usize = if has("--new-panel") { 2 } else if has("--new") { 1 } else { 0 };
    // Started from the menu (no options) or with --settings: show the settings
    // window. The login autostart uses --background and shows nothing.
    let want_prefs = has("--settings") || (new_req == 0 && !has("--background") && !has("--portable"));

    // Only one instance runs. A second one forwards its request and exits.
    let listener = match instance::listen() {
        Ok(l) => l,
        Err(()) => {
            let msg = if new_req == 2 {
                "new-panel"
            } else if new_req == 1 {
                "new"
            } else if has("--quit") {
                "quit"
            } else if want_prefs {
                "prefs"
            } else {
                ""
            };
            if !msg.is_empty() {
                instance::send(msg);
            }
            return;
        }
    };
    if has("--quit") {
        return;
    }
    ffi::x::x(); // opens the display, or explains why it can't
    // items put on the desktop by the quit button go back into their folders
    park::unpark();

    let a = app::App::init();
    drop::init();
    update::on_start();
    if new_req != 0 {
        a.new_folder_at_cursor(new_req == 2);
    } else if want_prefs {
        a.open_prefs();
    }

    // wake up when crow.ini or any folder changes on disk (hot reload, sync)
    let mut watcher = watch::Watcher::new(&util::from_w(&store::root()));
    run(a, &listener, &mut watcher);
}

fn run(a: &mut app::App, listener: &std::os::unix::net::UnixListener, watcher: &mut watch::Watcher) -> ! {
    let xfd = ffi::x::x().fd();
    let mut next_frame = 0.0f64;
    loop {
        if !ffi::pump() {
            quit();
        }
        if a.any_animating() {
            // While animating, render about once per display frame.
            let now = util::now_ms();
            if now >= next_frame {
                a.tick();
                ffi::x::x().flush();
                next_frame = now.max(next_frame) + 1000.0 / 60.0;
                if next_frame < now {
                    next_frame = now + 1000.0 / 60.0;
                }
            }
            let wait = (next_frame - util::now_ms()).max(0.0) as i32;
            wait_fds(&[xfd, listener_fd(listener), watcher.fd()], wait, a, listener, watcher);
            continue;
        }
        a.was_animating = false;
        // When idle, sleep until input, a timer, a request or a file change.
        let t = ffi::next_timer_ms().map_or(-1, |v| v.min(i32::MAX as u64) as i32);
        let woke = wait_fds(&[xfd, listener_fd(listener), watcher.fd()], t, a, listener, watcher);
        if !ffi::pump() {
            quit();
        }
        // Anything that woke us touches memory again. Trim it once things are
        // quiet, unless this wakeup was the trim itself.
        if woke {
            if a.trimmed {
                a.trimmed = false;
            } else {
                a.arm_idle();
            }
        }
    }
}

fn listener_fd(l: &std::os::unix::net::UnixListener) -> i32 {
    use std::os::fd::AsRawFd;
    l.as_raw_fd()
}

/// Waits up to `ms` (-1 forever) for any of the descriptors, then handles
/// requests from other instances and file changes. Returns whether anything
/// arrived.
fn wait_fds(fds: &[i32], ms: i32, a: &mut app::App, listener: &std::os::unix::net::UnixListener, watcher: &mut watch::Watcher) -> bool {
    // Events already read from the socket while waiting for replies mean
    // there is work right away, but requests from other processes (like a
    // quit) are still looked at, so a busy Crow can always be closed.
    let busy = ffi::has_pending() || ffi::x::x().buffered();
    ffi::watchdog(!busy && ms != 0);
    let mut p: Vec<libc::pollfd> = fds.iter().filter(|&&f| f >= 0).map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    let r = unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, if busy { 0 } else { ms }) };
    if r <= 0 {
        return busy;
    }
    for q in &p {
        if q.revents == 0 {
            continue;
        }
        if q.fd == listener_fd(listener) {
            for msg in instance::accept(listener) {
                handle_request(a, &msg);
            }
        } else if q.fd == watcher.fd() && watcher.drain() {
            a.reload();
            watcher.rewatch();
        }
    }
    true
}

fn handle_request(a: &mut app::App, msg: &str) {
    let h = a.msg;
    unsafe {
        match msg {
            "new" => {
                PostMessageW(h, app::WM_CROW_NEW, 1, 0);
            }
            "new-panel" => {
                PostMessageW(h, app::WM_CROW_NEW, 2, 0);
            }
            "prefs" => {
                PostMessageW(h, app::WM_CROW_PREFS, 0, 0);
            }
            "quit" => {
                // asked by another process (update, uninstall, --quit): items stay put
                KEEP_ITEMS = true;
                PostMessageW(h, app::WM_CROW_QUIT, 0, 0);
            }
            m => {
                if let Some(v) = m.strip_prefix("update ").and_then(|v| v.parse::<usize>().ok()) {
                    PostMessageW(h, app::WM_CROW_UPDATE, v, 0);
                }
            }
        }
    }
}

/// Set when Crow is closed by something other than the quit button.
static mut KEEP_ITEMS: bool = false;

fn quit() -> ! {
    let a = app::app();
    a.save();
    // The quit button: everything goes back on the desktop until Crow runs again.
    if !unsafe { KEEP_ITEMS } && !ffi::x::x().broken {
        park::park();
    }
    unsafe {
        for w in &a.wins {
            RevokeDragDrop(w.hwnd);
        }
    }
    ffi::x::x().flush();
    std::process::exit(0)
}

/// Counts live heap bytes (shown by CROW_DEBUG_TRIM=1).
pub struct Counting;
pub static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), std::sync::atomic::Ordering::Relaxed);
        std::alloc::System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        LIVE.fetch_sub(l.size(), std::sync::atomic::Ordering::Relaxed);
        std::alloc::System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n.wrapping_sub(l.size()), std::sync::atomic::Ordering::Relaxed);
        std::alloc::System.realloc(p, l, n)
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;
