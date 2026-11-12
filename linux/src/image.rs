//! Image sources on Linux, loaded on demand and released right after.
//!
//!  - PNG (what icon themes are made of) is decoded here.
//!  - SVG goes through Crow's own renderer.
//!  - Everything else (JPEG, WebP, GIF, BMP, ICO, TIFF...) and SVG files the
//!    renderer can't draw go to the desktop's own image library, gdk-pixbuf,
//!    the way Windows hands images to WIC. It runs in a short-lived child
//!    process, so the library never stays in Crow's memory.
//!
//! File and app icons come from the icon theme (see icons.rs).

use crate::gfx::fit_square;
use crate::util::*;
use alloc::vec::Vec;
use std::path::Path;

/// Loads any supported image file as a premultiplied square of s by s pixels.
pub fn load_file(path: &[u16], s: i32, tint: u32) -> Option<Vec<u32>> {
    // These paths come from crow.ini, which may arrive through sync, so only local files are allowed
    if !is_local_path(path) || !exists(path) {
        return None;
    }
    if ext_is(path, "svg") {
        let data = read_file_max(path, 4 << 20)?;
        if let Some(px) = crate::svg::render(&data, s, tint).filter(|p| visible(p)) {
            return Some(px);
        }
    }
    decode_path(&crate::ffi::lin_path(path), s)
}

fn visible(px: &[u32]) -> bool {
    px.iter().any(|&c| c >> 24 != 0)
}

/// Any image file (SVG included) as an s by s square.
pub fn decode_path(p: &Path, s: i32) -> Option<Vec<u32>> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > 64 << 20 {
        return None;
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "svg" && meta.len() <= 4 << 20 {
        let data = std::fs::read(p).ok()?;
        if let Some(px) = crate::svg::render(&data, s, 0).filter(|px| visible(px)) {
            return Some(px);
        }
    } else if let Some(px) = png(p, s) {
        return Some(px);
    }
    pixbuf(p, s)
}

/// A PNG file, or None when it isn't one.
fn png(p: &Path, s: i32) -> Option<Vec<u32>> {
    let f = std::fs::File::open(p).ok()?;
    let mut dec = png::Decoder::new_with_limits(std::io::BufReader::new(f), png::Limits { bytes: 128 << 20 });
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as i32, info.height as i32);
    let ch = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return None,
    };
    let px = premultiply(&buf, ch, w as usize, (w * h) as usize, info.line_size);
    finish(px, w, h, s)
}

/// Straight RGB(A) or gray rows to premultiplied ARGB pixels.
fn premultiply(b: &[u8], ch: usize, w: usize, n: usize, stride: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (y, x) = (i / w, i % w);
        let q = &b[y * stride + x * ch..y * stride + x * ch + ch];
        let (r, g, bl, a) = match ch {
            4 => (q[0], q[1], q[2], q[3]),
            3 => (q[0], q[1], q[2], 255),
            2 => (q[0], q[0], q[0], q[1]),
            _ => (q[0], q[0], q[0], 255),
        };
        out.push(crate::gfx::pm(0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | bl as u32, a as u32));
    }
    out
}

fn finish(px: Vec<u32>, w: i32, h: i32, s: i32) -> Option<Vec<u32>> {
    if w <= 0 || h <= 0 {
        return None;
    }
    if w == s && h == s {
        return Some(px);
    }
    Some(fit_square(&px, w, h, s))
}

// ------------------------------------------------------------ gdk-pixbuf
type PixbufNew = unsafe extern "C" fn(*const libc::c_char, i32, i32, i32, *mut *mut libc::c_void) -> *mut libc::c_void;
type PixbufInt = unsafe extern "C" fn(*mut libc::c_void) -> i32;
type PixbufPixels = unsafe extern "C" fn(*mut libc::c_void) -> *mut u8;

/// Decodes with gdk-pixbuf in a child process, which sends back width,
/// height and premultiplied pixels through a pipe.
fn pixbuf(p: &Path, s: i32) -> Option<Vec<u32>> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let path = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        return None;
    }
    if pid == 0 {
        // the child never returns into Crow
        unsafe {
            libc::close(fds[0]);
            let ok = child_decode(&path, s, fds[1]);
            libc::_exit(if ok { 0 } else { 1 });
        }
    }
    unsafe { libc::close(fds[1]) };
    let f = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    let mut data = Vec::new();
    let _ = f.take(8 + 4 * 1024 * 1024 * 4).read_to_end(&mut data);
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    if data.len() < 8 {
        return None;
    }
    let w = u32::from_le_bytes(data[0..4].try_into().ok()?) as i32;
    let h = u32::from_le_bytes(data[4..8].try_into().ok()?) as i32;
    if w <= 0 || h <= 0 || data.len() != 8 + (w * h) as usize * 4 {
        return None;
    }
    let px: Vec<u32> = data[8..].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    finish(px, w, h, s)
}

unsafe fn child_decode(path: &std::ffi::CStr, s: i32, out: i32) -> bool {
    let lib = libc::dlopen(c"libgdk_pixbuf-2.0.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
    if lib.is_null() {
        return false;
    }
    let sym = |n: &std::ffi::CStr| libc::dlsym(lib, n.as_ptr());
    let syms = [
        sym(c"gdk_pixbuf_new_from_file_at_scale"),
        sym(c"gdk_pixbuf_get_width"),
        sym(c"gdk_pixbuf_get_height"),
        sym(c"gdk_pixbuf_get_rowstride"),
        sym(c"gdk_pixbuf_get_n_channels"),
        sym(c"gdk_pixbuf_get_pixels"),
    ];
    if syms.iter().any(|p| p.is_null()) {
        return false;
    }
    let new: PixbufNew = core::mem::transmute(syms[0]);
    let int = |p: *mut libc::c_void| -> PixbufInt { core::mem::transmute(p) };
    let (width, height, stride, chans) = (int(syms[1]), int(syms[2]), int(syms[3]), int(syms[4]));
    let pixels: PixbufPixels = core::mem::transmute(syms[5]);
    // a bit more than needed, scaled down later for a crisper result
    let mut err = core::ptr::null_mut();
    let pb = new(path.as_ptr(), s * 2, s * 2, 1, &mut err);
    if pb.is_null() {
        return false;
    }
    let (w, h, rs, ch) = (width(pb), height(pb), stride(pb), chans(pb));
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 || !(ch == 3 || ch == 4) {
        return false;
    }
    let src = core::slice::from_raw_parts(pixels(pb), (rs * (h - 1) + w * ch) as usize);
    let px = premultiply(src, ch as usize, w as usize, (w * h) as usize, rs as usize);
    let mut buf = Vec::with_capacity(8 + px.len() * 4);
    buf.extend_from_slice(&(w as u32).to_le_bytes());
    buf.extend_from_slice(&(h as u32).to_le_bytes());
    for c in px {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    let mut off = 0;
    while off < buf.len() {
        let n = libc::write(out, buf[off..].as_ptr() as *const libc::c_void, buf.len() - off);
        if n <= 0 {
            return false;
        }
        off += n as usize;
    }
    true
}

/// Returns the icon or thumbnail a file manager shows for `path`, as an s by s square.
pub fn shell_icon(path: &[u16], s: i32) -> Option<Vec<u32>> {
    // ask for a bit more than needed so it scales down (crisper)
    let req = if s <= 48 { s * 2 } else { s };
    let px = crate::icons::for_path(&crate::ffi::lin_path(path), req)?;
    if req == s {
        return Some(px);
    }
    Some(fit_square(&px, req, req, s))
}
