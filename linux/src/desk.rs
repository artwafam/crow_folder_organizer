//! Keeps folders on the desktop layer: above the desktop and its icons, below
//! every normal app window. When the desktop itself is in front (a click on
//! the wallpaper or Super+D) the folders move to the top so they stay visible,
//! and they go back down as soon as an app gets focus. On X11 these are the
//! window manager's "below" and "above" bands (EWMH).

use crate::ffi::x::x;
use crate::ffi::*;
use x11rb::protocol::xproto::AtomEnum;

/// Windows asks Explorer for the window that holds the desktop icons; X11
/// needs no such window.
pub fn host() -> HWND {
    0
}

/// The desktop icon grid, when the desktop aligns icons to one. Linux file
/// managers don't share theirs, so folders snap to Crow's own grid.
pub fn icon_grid() -> Option<(i32, i32)> {
    None
}

/// Whether `h` is the desktop: the file manager's desktop window, nothing at
/// all, or the "show desktop" mode (reported as -1).
pub fn is_desktop(h: HWND) -> bool {
    if h == -1 || h == 0 {
        return true;
    }
    let xs = x();
    let types = xs.prop32(h as u32, xs.atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM);
    types.contains(&xs.atoms._NET_WM_WINDOW_TYPE_DESKTOP)
}

/// Rests `hwnd` on the desktop, below app windows.
pub fn sink(hwnd: HWND, _ours: &dyn Fn(HWND) -> bool) {
    win::set_layer(hwnd, -1);
}

pub fn set_topmost(hwnd: HWND, on: bool) {
    win::set_layer(hwnd, if on { 1 } else { 0 });
}

/// Brings an opening folder over the apps until it closes again.
pub fn raise(hwnd: HWND) {
    win::set_layer(hwnd, 1);
}
