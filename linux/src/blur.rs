//! Background blur drawn by the compositor. On X11 only KWin (KDE Plasma)
//! offers it to apps, through _KDE_NET_WM_BLUR_BEHIND_REGION. Other desktops
//! show the panels with their plain translucent background.

use crate::config::Global;
use crate::ffi::x::x;
use crate::folder::FolderWin;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
use x11rb::wrapper::ConnectionExt as _;

/// The blur follows the window itself, so there is nothing to restack.
pub fn restack(_f: &FolderWin) {}

/// Whether the compositor can blur behind windows.
pub fn system_allows() -> bool {
    static mut KWIN: Option<bool> = None;
    unsafe {
        *(*core::ptr::addr_of_mut!(KWIN)).get_or_insert_with(|| {
            std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")))
                || std::env::var("KDE_FULL_SESSION").is_ok_and(|v| v == "true")
        })
    }
}

pub fn apply(f: &mut FolderWin, on: bool, g: &Global) {
    let xs = x();
    let id = f.hwnd as u32;
    if id == 0 {
        return;
    }
    let atom = xs.atoms._KDE_NET_WM_BLUR_BEHIND_REGION;
    if !on || !system_allows() {
        if f.backdrop != 0 {
            let _ = xs.conn.delete_property(id, atom);
            f.backdrop = 0;
        }
        return;
    }
    let (x0, y0, w, h) = f.cur_rect();
    let inset = 1.0 * f.sc;
    let (x0, y0) = (x0 + inset, y0 + inset);
    let (w, h) = ((w - inset * 2.0).max(1.0), (h - inset * 2.0).max(1.0));
    let r = (g.radius as f32 * f.sc).min(w / 2.0).min(h / 2.0).max(0.0);
    // A rounded rectangle as rows: the corners step in, the middle is one block.
    let mut v: Vec<u32> = Vec::new();
    let rows = r.ceil() as i32;
    let mut push = |x: f32, y: f32, w: f32, h: f32| {
        if w >= 1.0 && h >= 1.0 {
            v.extend([x.round() as u32, y.round() as u32, w.round() as u32, h.round() as u32]);
        }
    };
    for i in 0..rows {
        let dy = r - i as f32 - 0.5;
        let dx = r - (r * r - dy * dy).max(0.0).sqrt();
        push(x0 + dx, y0 + i as f32, w - 2.0 * dx, 1.0);
        push(x0 + dx, y0 + h - 1.0 - i as f32, w - 2.0 * dx, 1.0);
    }
    push(x0, y0 + rows as f32, w, h - 2.0 * rows as f32);
    let _ = xs.conn.change_property32(PropMode::REPLACE, id, atom, AtomEnum::CARDINAL, &v);
    f.backdrop = 1; // remembers that a region is set
    xs.flush();
}
