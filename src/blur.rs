//! Background blur drawn by the compositor (DWM), so it costs us no memory.
//! DWM can't blur behind layered windows with per-pixel alpha, so each open
//! panel gets a plain helper window right beneath it. That window carries the
//! blur and follows the animated rounded shape.

use crate::config::Global;
use crate::ffi::*;
use crate::folder::FolderWin;

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;

fn accent(h: HWND, state: u32) {
    let mut a = ACCENT_POLICY { AccentState: state, AccentFlags: 0, GradientColor: 0, AnimationId: 0 };
    let mut d = WINCOMPATTRDATA { Attribute: WCA_ACCENT_POLICY, Data: &mut a as *mut _ as *mut _, SizeOfData: core::mem::size_of::<ACCENT_POLICY>() };
    unsafe { SetWindowCompositionAttribute(h, &mut d) };
}

fn is_top(h: HWND) -> bool {
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 }
}

/// Keeps the backdrop directly below its panel (and in the same band).
pub fn restack(f: &FolderWin) {
    if f.backdrop == 0 {
        return;
    }
    unsafe {
        let fl = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        if is_top(f.hwnd) != is_top(f.backdrop) {
            SetWindowPos(f.backdrop, if is_top(f.hwnd) { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, fl);
        }
        SetWindowPos(f.backdrop, f.hwnd, 0, 0, 0, 0, fl);
    }
}

/// Reads the Windows "Transparency effects" switch. When it is off, DWM paints
/// the blur as a solid color.
pub fn system_allows() -> bool {
    unsafe {
        let mut k: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(), 0, KEY_READ, &mut k) != 0 {
            return true;
        }
        let mut v: u32 = 1;
        let mut n = 4u32;
        let mut ty = 0u32;
        RegQueryValueExW(k, w!("EnableTransparency").as_ptr(), core::ptr::null_mut(), &mut ty, &mut v as *mut u32 as *mut u8, &mut n);
        RegCloseKey(k);
        v != 0
    }
}

pub fn apply(f: &mut FolderWin, on: bool, g: &Global) {
    unsafe {
        let on = on && (f.backdrop != 0 || system_allows());
        if !on {
            if f.backdrop != 0 {
                DestroyWindow(f.backdrop);
                f.backdrop = 0;
            }
            return;
        }
        let (x, y, w, h) = f.cur_rect();
        let inset = 1.0 * f.sc;
        let sx = (f.win.left as f32 + x + inset) as i32;
        let sy = (f.win.top as f32 + y + inset) as i32;
        let sw = (w - inset * 2.0).max(1.0) as i32;
        let sh = (h - inset * 2.0).max(1.0) as i32;
        let created = f.backdrop == 0;
        if created {
            let inst = GetModuleHandleW(core::ptr::null());
            f.backdrop = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, w!("CrowBackdrop").as_ptr(),
                core::ptr::null(), WS_POPUP, sx, sy, sw, sh, 0, 0, inst, core::ptr::null());
            if f.backdrop == 0 {
                return;
            }
            let m = MARGINS { l: -1, r: -1, t: -1, b: -1 };
            DwmExtendFrameIntoClientArea(f.backdrop, &m);
            let round: i32 = 2; // DWMWCP_ROUND gives native Windows 11 corners. Window regions would disable blur.
            DwmSetWindowAttribute(f.backdrop, 33, &round as *const i32 as _, 4);
            let dark: i32 = 1;
            DwmSetWindowAttribute(f.backdrop, 20, &dark as *const i32 as _, 4);
            if g.blur_kind == 1 {
                let v: i32 = 3; // DWMSBT_TRANSIENTWINDOW (acrylic)
                DwmSetWindowAttribute(f.backdrop, 38, &v as *const i32 as _, 4);
            } else {
                accent(f.backdrop, ACCENT_ENABLE_BLURBEHIND);
            }
        }
        let fl = SWP_NOACTIVATE | SWP_NOOWNERZORDER | if created { SWP_SHOWWINDOW } else { 0 };
        if is_top(f.hwnd) != is_top(f.backdrop) {
            SetWindowPos(f.backdrop, if is_top(f.hwnd) { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        SetWindowPos(f.backdrop, f.hwnd, sx, sy, sw, sh, fl);
    }
}
