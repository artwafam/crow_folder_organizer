//! Keeps our windows on the desktop layer, right above the window that hosts
//! the desktop icons and below every normal app window. When the desktop itself
//! gets focus (Win+D or a click on the wallpaper) the folders move to the top so
//! they stay visible, and they go back down as soon as an app gets focus.

use crate::ffi::*;
use crate::util::*;

fn class_is(h: HWND, name: &[u16]) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), 64) } as usize;
    weq_ic(&buf[..n], name)
}

/// The top-level window that contains SHELLDLL_DefView (Progman or a WorkerW).
pub fn host() -> HWND {
    unsafe {
        let prog = FindWindowW(w!("Progman").as_ptr(), core::ptr::null());
        if prog != 0 && FindWindowExW(prog, 0, w!("SHELLDLL_DefView").as_ptr(), core::ptr::null()) != 0 {
            return prog;
        }
        let mut w = 0;
        loop {
            w = FindWindowExW(0, w, w!("WorkerW").as_ptr(), core::ptr::null());
            if w == 0 {
                break;
            }
            if FindWindowExW(w, 0, w!("SHELLDLL_DefView").as_ptr(), core::ptr::null()) != 0 {
                return w;
            }
        }
        prog
    }
}

pub fn is_desktop(h: HWND) -> bool {
    h != 0 && (class_is(h, w!("WorkerW")) || class_is(h, w!("Progman")))
}

/// Places `hwnd` directly above the desktop host, skipping our own windows.
pub fn sink(hwnd: HWND, ours: &dyn Fn(HWND) -> bool) {
    unsafe {
        let h = host();
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        if h == 0 {
            SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, flags);
            return;
        }
        let mut above = GetWindow(h, GW_HWNDPREV);
        while above != 0 && (above == hwnd || ours(above)) {
            above = GetWindow(above, GW_HWNDPREV);
        }
        let topmost = above != 0 && (GetWindowLongPtrW(above, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST) != 0;
        let after = if above == 0 || topmost { HWND_TOP } else { above };
        SetWindowPos(hwnd, after, 0, 0, 0, 0, flags);
    }
}

pub fn set_topmost(hwnd: HWND, on: bool) {
    unsafe {
        SetWindowPos(hwnd, if on { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    }
}

pub fn raise(hwnd: HWND) {
    unsafe {
        SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    }
}
