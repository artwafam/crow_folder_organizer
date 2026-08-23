//! Image sources. Everything is loaded on demand and released right after.
//! Bitmap files (png, jpg, bmp, ico, gif, webp and others) go through WIC, SVG
//! goes through our own renderer, and shell icons and thumbnails come from
//! IShellItemImageFactory.

use crate::ffi::*;
use crate::gfx::{fit_square, Canvas};
use crate::util::*;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::null_mut;

const CLSID_WIC_FACTORY: GUID = guid(0xcacaf262, 0x9370, 0x4615, [0xa1, 0x3b, 0x9f, 0x55, 0x39, 0xda, 0x4c, 0x0a]);
const IID_WIC_FACTORY: GUID = guid(0xec5ec8a9, 0xc395, 0x4314, [0x9c, 0x77, 0x54, 0xd7, 0xa9, 0x35, 0xff, 0x70]);
const PBGRA: GUID = guid(0x6fddc324, 0x4e03, 0x4bfe, [0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9, 0x10]);
const IID_IMAGE_FACTORY: GUID = guid(0xbcc18b79, 0xba16, 0x442f, [0x80, 0xc4, 0x8a, 0x59, 0xc3, 0x0c, 0x46, 0x3b]);

/// Loads any supported image file as a premultiplied square of s by s pixels.
pub fn load_file(path: &[u16], s: i32, tint: u32) -> Option<Vec<u32>> {
    // These paths come from crow.ini, which may arrive through sync, so only local files are allowed
    if !is_local_path(path) || !exists(path) {
        return None;
    }
    if ext_is(path, "svg") {
        let data = read_file_max(path, 4 << 20)?;
        return crate::svg::render(&data, s, tint);
    }
    wic_load(path, s)
}

struct Com(*mut c_void);
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { com_release(self.0) }
    }
}

fn wic_load(path: &[u16], s: i32) -> Option<Vec<u32>> {
    if file_size(path)? > 64 << 20 {
        return None;
    }
    unsafe {
        let mut f = null_mut();
        if CoCreateInstance(&CLSID_WIC_FACTORY, null_mut(), CLSCTX_INPROC_SERVER, &IID_WIC_FACTORY, &mut f) < 0 {
            return None;
        }
        let f = Com(f);
        let p = wz(path);
        let mut dec = null_mut();
        let create_dec: unsafe extern "system" fn(*mut c_void, PCWSTR, *const GUID, u32, u32, *mut *mut c_void) -> HRESULT =
            core::mem::transmute(vt(f.0, 3));
        if create_dec(f.0, p.as_ptr(), core::ptr::null(), GENERIC_READ, 0, &mut dec) < 0 {
            return None;
        }
        let dec = Com(dec);
        // pick the largest frame (ICO files hold many sizes)
        let frame_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT = core::mem::transmute(vt(dec.0, 12));
        let get_frame: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT = core::mem::transmute(vt(dec.0, 13));
        let mut n = 0u32;
        frame_count(dec.0, &mut n);
        let mut best: Option<(Com, u32, u32)> = None;
        for i in 0..n.clamp(1, 32) {
            let mut fr = null_mut();
            if get_frame(dec.0, i, &mut fr) < 0 {
                break;
            }
            let fr = Com(fr);
            let get_size: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) -> HRESULT = core::mem::transmute(vt(fr.0, 3));
            let (mut w, mut h) = (0u32, 0u32);
            get_size(fr.0, &mut w, &mut h);
            if best.as_ref().map_or(true, |b| w * h > b.1 * b.2) {
                best = Some((fr, w, h));
            }
            if n > 1 && w as i32 >= s * 2 {
                break;
            }
        }
        let (frame, w, h) = best?;
        // refuse decompression bombs
        if w == 0 || h == 0 || w > 16384 || h > 16384 || (w as u64) * (h as u64) > 64_000_000 {
            return None;
        }
        // convert to premultiplied BGRA
        let mut conv = null_mut();
        let create_conv: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT = core::mem::transmute(vt(f.0, 10));
        if create_conv(f.0, &mut conv) < 0 {
            return None;
        }
        let conv = Com(conv);
        let init: unsafe extern "system" fn(*mut c_void, *mut c_void, *const GUID, u32, *mut c_void, f64, u32) -> HRESULT =
            core::mem::transmute(vt(conv.0, 8));
        if init(conv.0, frame.0, &PBGRA, 0, null_mut(), 0.0, 0) < 0 {
            return None;
        }
        // scale to fit with Fant (high-quality downscale) when needed
        let k = (s as f32 / w as f32).min(s as f32 / h as f32);
        let (dw, dh) = (((w as f32 * k) + 0.5).max(1.0) as u32, ((h as f32 * k) + 0.5).max(1.0) as u32);
        let mut srcobj = Com(conv.0);
        core::mem::forget(conv); // ownership moved into srcobj
        if dw != w || dh != h {
            let mut sc = null_mut();
            let create_sc: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT = core::mem::transmute(vt(f.0, 11));
            if create_sc(f.0, &mut sc) >= 0 {
                let sinit: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u32, u32) -> HRESULT = core::mem::transmute(vt(sc, 8));
                let mode = if k < 1.0 { 3 } else { 4 }; // Fant / HighQualityCubic
                if sinit(sc, srcobj.0, dw, dh, mode) >= 0 {
                    srcobj = Com(sc);
                } else {
                    com_release(sc);
                }
            }
        }
        let get_size: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) -> HRESULT = core::mem::transmute(vt(srcobj.0, 3));
        let (mut cw, mut ch) = (0u32, 0u32);
        get_size(srcobj.0, &mut cw, &mut ch);
        let mut buf = vec![0u32; (cw * ch) as usize];
        let copy: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut u8) -> HRESULT = core::mem::transmute(vt(srcobj.0, 7));
        if copy(srcobj.0, core::ptr::null(), cw * 4, cw * ch * 4, buf.as_mut_ptr() as *mut u8) < 0 {
            return None;
        }
        drop(srcobj);
        drop(frame);
        if cw as i32 == s && ch as i32 == s {
            return Some(buf);
        }
        let mut c = Canvas::mem(s, s);
        c.blit(&buf, cw as i32, ch as i32, ((s - cw as i32) / 2) as f32, ((s - ch as i32) / 2) as f32, cw as f32, ch as f32, 256);
        Some(c.into_vec())
    }
}

/// Returns the icon or thumbnail Explorer shows for `path`, as an s by s square.
pub fn shell_icon(path: &[u16], s: i32) -> Option<Vec<u32>> {
    unsafe {
        let p = wz(path);
        let mut f = null_mut();
        if SHCreateItemFromParsingName(p.as_ptr(), null_mut(), &IID_IMAGE_FACTORY, &mut f) < 0 || f.is_null() {
            return None;
        }
        let f = Com(f);
        let get: unsafe extern "system" fn(*mut c_void, SIZE, u32, *mut HBITMAP) -> HRESULT = core::mem::transmute(vt(f.0, 3));
        let mut hb: HBITMAP = 0;
        // ask for a bit more than needed so we downsample (crisper)
        let req = if s <= 48 { s * 2 } else { s };
        if get(f.0, SIZE { cx: req, cy: req }, 0, &mut hb) < 0 || hb == 0 {
            if get(f.0, SIZE { cx: s, cy: s }, 4, &mut hb) < 0 || hb == 0 {
                return None;
            }
        }
        let px = hbitmap_pixels(hb);
        DeleteObject(hb);
        let (w, h, px) = px?;
        Some(fit_square(&px, w, h, s))
    }
}

unsafe fn hbitmap_pixels(hb: HBITMAP) -> Option<(i32, i32, Vec<u32>)> {
    let mut bm: BITMAP = core::mem::zeroed();
    if GetObjectW(hb, core::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut c_void) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
        return None;
    }
    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = core::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    let mut px = vec![0u32; (w * h) as usize];
    let dc = CreateCompatibleDC(0);
    let r = GetDIBits(dc, hb, 0, h as u32, px.as_mut_ptr() as *mut c_void, &mut bi, DIB_RGB_COLORS);
    DeleteDC(dc);
    if r == 0 {
        return None;
    }
    let has_alpha = px.iter().any(|&c| c >> 24 != 0);
    if !has_alpha {
        for c in px.iter_mut() {
            *c |= 0xFF00_0000;
        }
    } else {
        let premul = px.iter().all(|&c| {
            let a = c >> 24;
            ((c >> 16) & 255) <= a && ((c >> 8) & 255) <= a && (c & 255) <= a
        });
        if !premul {
            for c in px.iter_mut() {
                *c = crate::gfx::pm(*c, 255);
            }
        }
    }
    Some((w, h, px))
}
