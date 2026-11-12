//! The Win32 subset Crow uses, implemented for Linux and X11.
//!
//! The shared code in ../src talks to Windows through `crate::ffi`. On Linux
//! that module is this one: the same types and constants (copied from
//! ../src/ffi.rs by build.rs) and the same functions, built on X11, the file
//! system and a small font rasterizer. HWND values are X window ids.
#![allow(non_snake_case, non_camel_case_types, dead_code, clippy::upper_case_acronyms, clippy::missing_safety_doc)]

use core::ffi::c_void;

include!(concat!(env!("OUT_DIR"), "/ffi_types.rs"));

mod font;
mod gdi;
mod kernel;
mod keys;
mod shell;
pub mod win;
pub mod x;

pub use gdi::*;
pub use kernel::*;
pub use shell::*;
pub use win::*;

/// Reads a NUL-terminated UTF-16 string.
pub(crate) unsafe fn wstr(p: PCWSTR) -> Vec<u16> {
    if p.is_null() {
        return Vec::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::slice::from_raw_parts(p, n).to_vec()
}

pub(crate) unsafe fn wstring(p: PCWSTR) -> String {
    String::from_utf16_lossy(&wstr(p))
}

/// A Windows-style path as a Linux one. The shared code joins paths with `\`,
/// which never appears in the paths Crow builds on Linux.
pub(crate) fn lin_path(w: &[u16]) -> std::path::PathBuf {
    let s = String::from_utf16_lossy(crate::util::trim_nul(w)).replace('\\', "/");
    std::path::PathBuf::from(s)
}

pub(crate) unsafe fn lin_pathp(p: PCWSTR) -> std::path::PathBuf {
    lin_path(&wstr(p))
}

/// Copies `s` into a Win32 output buffer of `cap` units, NUL-terminated.
/// Returns the length without the NUL, or the size needed when it doesn't fit.
pub(crate) unsafe fn put_w(s: &[u16], buf: PWSTR, cap: u32) -> u32 {
    if buf.is_null() || (s.len() + 1) as u32 > cap {
        return (s.len() + 1) as u32;
    }
    core::ptr::copy_nonoverlapping(s.as_ptr(), buf, s.len());
    *buf.add(s.len()) = 0;
    s.len() as u32
}
