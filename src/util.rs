//! Small helpers for UTF-16 strings, float math without libm, files and hashing.
#![allow(dead_code)]

use crate::ffi::*;
use alloc::string::String;
use alloc::vec::Vec;

// ------------------------------------------------------------- wide strings
pub const fn utf16_len(s: &str) -> usize {
    let b = s.as_bytes();
    let (mut i, mut n) = (0, 0);
    while i < b.len() {
        let c = b[i];
        i += if c < 0x80 { 1 } else if c < 0xE0 { 2 } else if c < 0xF0 { 3 } else { n += 1; 4 };
        n += 1;
    }
    n
}

pub const fn utf16_arr<const N: usize>(s: &str) -> [u16; N] {
    let b = s.as_bytes();
    let mut out = [0u16; N];
    let (mut i, mut o) = (0, 0);
    while i < b.len() {
        let c = b[i] as u32;
        let (cp, l) = if c < 0x80 {
            (c, 1)
        } else if c < 0xE0 {
            (((c & 0x1F) << 6) | (b[i + 1] as u32 & 0x3F), 2)
        } else if c < 0xF0 {
            (((c & 0x0F) << 12) | ((b[i + 1] as u32 & 0x3F) << 6) | (b[i + 2] as u32 & 0x3F), 3)
        } else {
            (((c & 0x07) << 18) | ((b[i + 1] as u32 & 0x3F) << 12) | ((b[i + 2] as u32 & 0x3F) << 6) | (b[i + 3] as u32 & 0x3F), 4)
        };
        if cp >= 0x10000 {
            let v = cp - 0x10000;
            out[o] = (0xD800 + (v >> 10)) as u16;
            out[o + 1] = (0xDC00 + (v & 0x3FF)) as u16;
            o += 2;
        } else {
            out[o] = cp as u16;
            o += 1;
        }
        i += l;
    }
    out
}

/// Compile-time, NUL-terminated UTF-16 literal.
#[macro_export]
macro_rules! w {
    ($s:expr) => {{
        const S: &str = $s;
        const N: usize = $crate::util::utf16_len(S) + 1;
        const W: [u16; N] = $crate::util::utf16_arr::<N>(S);
        &W
    }};
}

pub type WString = Vec<u16>;

/// Returns a NUL-terminated copy.
pub fn wz(s: &[u16]) -> WString {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(trim_nul(s));
    v.push(0);
    v
}
pub fn trim_nul(s: &[u16]) -> &[u16] {
    match s.iter().position(|&c| c == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}
pub fn to_w(s: &str) -> WString {
    s.encode_utf16().collect()
}
pub fn from_w(s: &[u16]) -> String {
    core::char::decode_utf16(trim_nul(s).iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}
pub fn wcat(parts: &[&[u16]]) -> WString {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(trim_nul(p));
    }
    v
}
pub fn weq_ic(a: &[u16], b: &[u16]) -> bool {
    let (a, b) = (trim_nul(a), trim_nul(b));
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| lower(*x) == lower(*y))
}
#[inline]
pub fn lower(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) { c + 32 } else { c }
}
/// File name part of a path.
pub fn file_name(p: &[u16]) -> &[u16] {
    let p = trim_nul(p);
    match p.iter().rposition(|&c| c == b'\\' as u16 || c == b'/' as u16) {
        Some(i) => &p[i + 1..],
        None => p,
    }
}
pub fn parent_dir(p: &[u16]) -> &[u16] {
    let p = trim_nul(p);
    match p.iter().rposition(|&c| c == b'\\' as u16 || c == b'/' as u16) {
        Some(i) => &p[..i],
        None => &p[..0],
    }
}
/// Lower-case extension without the dot.
pub fn ext_lower(p: &[u16]) -> WString {
    let n = file_name(p);
    match n.iter().rposition(|&c| c == b'.' as u16) {
        Some(i) => n[i + 1..].iter().map(|&c| lower(c)).collect(),
        None => Vec::new(),
    }
}
pub fn stem(n: &[u16]) -> &[u16] {
    let n = file_name(n);
    match n.iter().rposition(|&c| c == b'.' as u16) {
        Some(i) if i > 0 => &n[..i],
        _ => n,
    }
}
pub fn ext_is(p: &[u16], e: &str) -> bool {
    let x = ext_lower(p);
    x.len() == e.len() && x.iter().zip(e.bytes()).all(|(a, b)| *a == b as u16)
}

pub fn fmt_int(mut v: i64, out: &mut String) {
    if v < 0 {
        out.push('-');
        v = -v;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for &b in &buf[i..] {
        out.push(b as char);
    }
}
pub fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, d) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    if d.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for b in d.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add((b - b'0') as i64);
    }
    Some(if neg { -v } else { v })
}
pub fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        b'A'..=b'F' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

// --------------------------------------------------------------------- math
#[inline]
pub fn sqrt(x: f32) -> f32 {
    unsafe {
        use core::arch::x86_64::*;
        _mm_cvtss_f32(_mm_sqrt_ss(_mm_set_ss(x)))
    }
}
#[inline]
pub fn abs(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7FFF_FFFF)
}
#[inline]
pub fn floor(x: f32) -> f32 {
    let t = x as i32 as f32;
    if t > x { t - 1.0 } else { t }
}
#[inline]
pub fn ceil(x: f32) -> f32 {
    let t = x as i32 as f32;
    if t < x { t + 1.0 } else { t }
}
/// Fractional part, always in [0, 1).
#[inline]
pub fn fract(x: f32) -> f32 {
    x - floor(x)
}
#[inline]
pub fn round(x: f32) -> f32 {
    floor(x + 0.5)
}
#[inline]
pub fn clamp(x: f32, a: f32, b: f32) -> f32 {
    if x < a { a } else if x > b { b } else { x }
}
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub const PI: f32 = core::f32::consts::PI;

/// Sine using range reduction and a 7th order polynomial. The error stays below 1e-5.
pub fn sin(x: f32) -> f32 {
    let mut x = x - (2.0 * PI) * floor(x / (2.0 * PI) + 0.5);
    // x is now in [-pi, pi], fold it into [-pi/2, pi/2]
    if x > PI / 2.0 {
        x = PI - x;
    } else if x < -PI / 2.0 {
        x = -PI - x;
    }
    let x2 = x * x;
    x * (1.0 + x2 * (-0.166_666_67 + x2 * (0.008_333_31 + x2 * (-0.000_198_41 + x2 * 0.000_002_7))))
}
#[inline]
pub fn cos(x: f32) -> f32 {
    sin(x + PI / 2.0)
}
pub fn atan2(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (abs(x), abs(y));
    let swap = ay > ax;
    let z = if swap { ax / ay } else { ay / ax };
    let z2 = z * z;
    let mut r = z * (0.999_866 + z2 * (-0.330_299_5 + z2 * (0.180_141 + z2 * (-0.085_133 + z2 * 0.020_835_1))));
    if swap {
        r = PI / 2.0 - r;
    }
    if x < 0.0 {
        r = PI - r;
    }
    if y < 0.0 { -r } else { r }
}

// easing
pub fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}
pub fn ease_out_quart(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u * u
}
pub fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { let u = -2.0 * t + 2.0; 1.0 - u * u * u / 2.0 }
}
/// Slight overshoot, for a "springy" open.
pub fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.25;
    let c3 = c1 + 1.0;
    let u = t - 1.0;
    1.0 + c3 * u * u * u + c1 * u * u
}

// --------------------------------------------------------------- timing
pub fn now_ms() -> f64 {
    unsafe {
        static mut F: i64 = 0;
        let f = &mut *core::ptr::addr_of_mut!(F);
        if *f == 0 {
            QueryPerformanceFrequency(f);
        }
        let mut c = 0i64;
        QueryPerformanceCounter(&mut c);
        c as f64 * 1000.0 / *f as f64
    }
}

// ---------------------------------------------------------------- files
pub fn read_file(path: &[u16]) -> Option<Vec<u8>> {
    unsafe {
        let p = wz(path);
        let h = CreateFileW(p.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                            core::ptr::null(), OPEN_EXISTING, 0, 0);
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut sz = 0i64;
        GetFileSizeEx(h, &mut sz);
        let mut v: Vec<u8> = Vec::with_capacity(sz as usize);
        let mut rd = 0u32;
        let ok = ReadFile(h, v.as_mut_ptr() as _, sz as u32, &mut rd, core::ptr::null_mut());
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        v.set_len(rd as usize);
        Some(v)
    }
}
/// Like read_file but refuses files larger than `max` bytes.
pub fn read_file_max(path: &[u16], max: u64) -> Option<Vec<u8>> {
    if file_size(path)? > max {
        return None;
    }
    read_file(path)
}
pub fn file_size(path: &[u16]) -> Option<u64> {
    unsafe {
        let mut d = WIN32_FILE_ATTRIBUTE_DATA::default();
        if GetFileAttributesExW(wz(path).as_ptr(), 0, &mut d) == 0 {
            return None;
        }
        Some(((d.size_hi as u64) << 32) | d.size_lo as u64)
    }
}
/// Accepts only plain local paths such as "C:\...". Network (UNC), device and
/// relative paths are rejected, because a synced crow.ini must never make us
/// open a remote share. Doing so could leak the user's NTLM credentials.
pub fn is_local_path(p: &[u16]) -> bool {
    let p = trim_nul(p);
    p.len() >= 3
        && (p[0] | 0x20) >= b'a' as u16
        && (p[0] | 0x20) <= b'z' as u16
        && p[1] == b':' as u16
        && (p[2] == b'\\' as u16 || p[2] == b'/' as u16)
}

pub fn write_file(path: &[u16], data: &[u8]) -> bool {
    unsafe {
        let p = wz(path);
        let h = CreateFileW(p.as_ptr(), GENERIC_WRITE, FILE_SHARE_READ, core::ptr::null(), CREATE_ALWAYS, 0, 0);
        if h == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut wr = 0u32;
        let ok = WriteFile(h, data.as_ptr() as _, data.len() as u32, &mut wr, core::ptr::null_mut());
        CloseHandle(h);
        ok != 0 && wr as usize == data.len()
    }
}
pub fn exists(path: &[u16]) -> bool {
    unsafe { GetFileAttributesW(wz(path).as_ptr()) != INVALID_FILE_ATTRIBUTES }
}
pub fn is_dir(path: &[u16]) -> bool {
    let a = unsafe { GetFileAttributesW(wz(path).as_ptr()) };
    a != INVALID_FILE_ATTRIBUTES && a & FILE_ATTRIBUTE_DIRECTORY != 0
}
pub fn mkdir(path: &[u16]) {
    unsafe { CreateDirectoryW(wz(path).as_ptr(), core::ptr::null()) };
}
pub fn mtime(path: &[u16]) -> u64 {
    unsafe {
        let mut d = WIN32_FILE_ATTRIBUTE_DATA::default();
        if GetFileAttributesExW(wz(path).as_ptr(), 0, &mut d) == 0 {
            return 0;
        }
        ((d.mtime.hi as u64) << 32) | d.mtime.lo as u64
    }
}
/// Lists the names of the files and folders in `dir`, leaving out hidden ones.
pub fn list_files(dir: &[u16]) -> Vec<WString> {
    let mut out = Vec::new();
    unsafe {
        let pat = wz(&wcat(&[dir, w!("\\*")]));
        let mut fd: WIN32_FIND_DATAW = core::mem::zeroed();
        let h = FindFirstFileW(pat.as_ptr(), &mut fd);
        if h == INVALID_HANDLE_VALUE {
            return out;
        }
        loop {
            let name = trim_nul(&fd.cFileName);
            if fd.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN == 0 && name != [46u16] && name != [46u16, 46] {
                out.push(name.to_vec());
            }
            if FindNextFileW(h, &mut fd) == 0 {
                break;
            }
        }
        FindClose(h);
    }
    out
}
pub fn env(name: &[u16]) -> WString {
    unsafe {
        let mut buf = [0u16; 520];
        let n = GetEnvironmentVariableW(name.as_ptr(), buf.as_mut_ptr(), 520) as usize;
        buf[..n.min(520)].to_vec()
    }
}
pub fn expand_env(s: &[u16]) -> WString {
    unsafe {
        let src = wz(s);
        let mut buf = [0u16; 1024];
        let n = ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), 1024) as usize;
        if n == 0 || n > 1024 {
            return trim_nul(s).to_vec();
        }
        trim_nul(&buf[..n]).to_vec()
    }
}

// --------------------------------------------------------------- hashing
pub fn fnv(h: u64, bytes: &[u8]) -> u64 {
    let mut h = if h == 0 { 0xcbf2_9ce4_8422_2325 } else { h };
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
pub fn fnv_w(h: u64, s: &[u16]) -> u64 {
    let mut h = if h == 0 { 0xcbf2_9ce4_8422_2325 } else { h };
    for &c in trim_nul(s) {
        let c = lower(c);
        for b in [c as u8, (c >> 8) as u8] {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}
