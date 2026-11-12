//! Files, environment, time, memory and processes.

use super::*;
use std::collections::HashMap;
use std::io::{Read, Write};

enum Handle {
    File(std::fs::File),
    Find(std::vec::IntoIter<(String, bool, bool)>),
}

struct Kernel {
    handles: HashMap<isize, Handle>,
    next: isize,
    globals: HashMap<isize, Vec<u8>>,
    last_error: u32,
}

static mut K: Option<Kernel> = None;

fn k() -> &'static mut Kernel {
    unsafe {
        let k = &mut *core::ptr::addr_of_mut!(K);
        k.get_or_insert_with(|| Kernel { handles: HashMap::new(), next: 0x1000, globals: HashMap::new(), last_error: 0 })
    }
}

fn add(h: Handle) -> isize {
    let k = k();
    k.next += 4;
    k.handles.insert(k.next, h);
    k.next
}

fn set_err(e: &std::io::Error) {
    k().last_error = match e.kind() {
        std::io::ErrorKind::PermissionDenied => ERROR_ACCESS_DENIED,
        std::io::ErrorKind::AlreadyExists => ERROR_ALREADY_EXISTS,
        _ => 2,
    };
}

pub(crate) fn set_last_error(e: u32) {
    k().last_error = e;
}

pub unsafe extern "system" fn GetLastError() -> u32 {
    k().last_error
}

// ------------------------------------------------------------------- files
pub unsafe extern "system" fn CreateFileW(n: PCWSTR, acc: u32, _share: u32, _sa: *const c_void, disp: u32, _flags: u32, _tmpl: HANDLE) -> HANDLE {
    let p = lin_pathp(n);
    let mut o = std::fs::OpenOptions::new();
    o.read(acc & GENERIC_READ != 0);
    if acc & GENERIC_WRITE != 0 {
        o.write(true);
    }
    match disp {
        CREATE_ALWAYS => {
            o.create(true).truncate(true);
        }
        1 => {
            o.create_new(true);
        }
        4 => {
            o.create(true);
        }
        _ => {}
    }
    match o.open(&p) {
        Ok(f) => {
            if f.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                k().last_error = ERROR_ACCESS_DENIED;
                return INVALID_HANDLE_VALUE;
            }
            add(Handle::File(f))
        }
        Err(e) => {
            set_err(&e);
            INVALID_HANDLE_VALUE
        }
    }
}

pub unsafe extern "system" fn ReadFile(h: HANDLE, buf: *mut c_void, n: u32, read: *mut u32, _ov: *mut c_void) -> BOOL {
    let Some(Handle::File(f)) = k().handles.get_mut(&h) else { return 0 };
    let b = core::slice::from_raw_parts_mut(buf as *mut u8, n as usize);
    let mut total = 0;
    while total < b.len() {
        match f.read(&mut b[total..]) {
            Ok(0) => break,
            Ok(r) => total += r,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                set_err(&e);
                return 0;
            }
        }
    }
    if !read.is_null() {
        *read = total as u32;
    }
    1
}

pub unsafe extern "system" fn WriteFile(h: HANDLE, buf: *const c_void, n: u32, written: *mut u32, _ov: *mut c_void) -> BOOL {
    let Some(Handle::File(f)) = k().handles.get_mut(&h) else { return 0 };
    let b = core::slice::from_raw_parts(buf as *const u8, n as usize);
    match f.write_all(b) {
        Ok(()) => {
            if !written.is_null() {
                *written = n;
            }
            1
        }
        Err(e) => {
            set_err(&e);
            0
        }
    }
}

pub unsafe extern "system" fn GetFileSizeEx(h: HANDLE, sz: *mut i64) -> BOOL {
    let Some(Handle::File(f)) = k().handles.get(&h) else { return 0 };
    match f.metadata() {
        Ok(m) => {
            *sz = m.len() as i64;
            1
        }
        Err(_) => 0,
    }
}

pub unsafe extern "system" fn CloseHandle(h: HANDLE) -> BOOL {
    k().handles.remove(&h).is_some() as BOOL
}

pub unsafe extern "system" fn CreateDirectoryW(n: PCWSTR, _sa: *const c_void) -> BOOL {
    match std::fs::create_dir(lin_pathp(n)) {
        Ok(()) => 1,
        Err(e) => {
            set_err(&e);
            0
        }
    }
}

pub unsafe extern "system" fn RemoveDirectoryW(n: PCWSTR) -> BOOL {
    std::fs::remove_dir(lin_pathp(n)).is_ok() as BOOL
}

pub unsafe extern "system" fn DeleteFileW(n: PCWSTR) -> BOOL {
    std::fs::remove_file(lin_pathp(n)).is_ok() as BOOL
}

pub unsafe extern "system" fn MoveFileExW(a: PCWSTR, b: PCWSTR, f: u32) -> BOOL {
    let (a, b) = (lin_pathp(a), lin_pathp(b));
    if f & MOVEFILE_REPLACE_EXISTING == 0 && std::fs::symlink_metadata(&b).is_ok() {
        k().last_error = ERROR_ALREADY_EXISTS;
        return 0;
    }
    match std::fs::rename(&a, &b) {
        Ok(()) => 1,
        Err(e) => {
            set_err(&e);
            0
        }
    }
}

fn attrs(p: &std::path::Path) -> Option<(u32, std::fs::Metadata)> {
    // Links count as what they point to, but a broken link still exists.
    let m = std::fs::metadata(p).or_else(|_| std::fs::symlink_metadata(p)).ok()?;
    let mut a = if m.is_dir() { FILE_ATTRIBUTE_DIRECTORY } else { 0x80 };
    if p.file_name().is_some_and(|n| n.as_encoded_bytes().first() == Some(&b'.')) {
        a |= FILE_ATTRIBUTE_HIDDEN;
    }
    Some((a, m))
}

fn filetime(t: std::io::Result<std::time::SystemTime>) -> FILETIME {
    let d = t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).unwrap_or_default();
    let v = (d.as_secs() + 11_644_473_600) * 10_000_000 + d.subsec_nanos() as u64 / 100;
    FILETIME { lo: v as u32, hi: (v >> 32) as u32 }
}

pub unsafe extern "system" fn GetFileAttributesW(n: PCWSTR) -> u32 {
    attrs(&lin_pathp(n)).map_or(INVALID_FILE_ATTRIBUTES, |a| a.0)
}

pub unsafe extern "system" fn GetFileAttributesExW(n: PCWSTR, _lvl: u32, d: *mut WIN32_FILE_ATTRIBUTE_DATA) -> BOOL {
    let Some((a, m)) = attrs(&lin_pathp(n)) else { return 0 };
    let d = &mut *d;
    d.attr = a;
    d.size_lo = m.len() as u32;
    d.size_hi = (m.len() >> 32) as u32;
    d.mtime = filetime(m.modified());
    d.atime = filetime(m.accessed());
    d.ctime = filetime(m.created().or_else(|_| m.modified()));
    1
}

pub unsafe extern "system" fn SetFileAttributesW(_n: PCWSTR, _a: u32) -> BOOL {
    1
}

unsafe fn fill_find(d: *mut WIN32_FIND_DATAW, e: &(String, bool, bool)) {
    let d = &mut *d;
    d.dwFileAttributes = if e.1 { FILE_ATTRIBUTE_DIRECTORY } else { 0x80 } | if e.2 { FILE_ATTRIBUTE_HIDDEN } else { 0 };
    let w: Vec<u16> = e.0.encode_utf16().take(259).collect();
    d.cFileName = [0; 260];
    d.cFileName[..w.len()].copy_from_slice(&w);
}

/// Only the patterns the shared code uses: `dir\*` and an exact path.
pub unsafe extern "system" fn FindFirstFileW(n: PCWSTR, d: *mut WIN32_FIND_DATAW) -> HANDLE {
    let p = lin_pathp(n);
    let s = p.to_string_lossy();
    let mut list: Vec<(String, bool, bool)> = Vec::new();
    if let Some(dir) = s.strip_suffix("/*") {
        let Ok(rd) = std::fs::read_dir(dir) else { return INVALID_HANDLE_VALUE };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
            let hidden = name.starts_with('.');
            list.push((name, is_dir, hidden));
        }
        list.sort();
    } else {
        let Some((a, _)) = attrs(&p) else { return INVALID_HANDLE_VALUE };
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        list.push((name, a & FILE_ATTRIBUTE_DIRECTORY != 0, a & FILE_ATTRIBUTE_HIDDEN != 0));
    }
    let mut it = list.into_iter();
    let Some(first) = it.next() else { return INVALID_HANDLE_VALUE };
    fill_find(d, &first);
    add(Handle::Find(it))
}

pub unsafe extern "system" fn FindNextFileW(h: HANDLE, d: *mut WIN32_FIND_DATAW) -> BOOL {
    let Some(Handle::Find(it)) = k().handles.get_mut(&h) else { return 0 };
    match it.next() {
        Some(e) => {
            fill_find(d, &e);
            1
        }
        None => 0,
    }
}

pub unsafe extern "system" fn FindClose(h: HANDLE) -> BOOL {
    CloseHandle(h)
}

// ------------------------------------------------------------- environment
fn env_w(name: &[u16]) -> Option<Vec<u16>> {
    let n = String::from_utf16_lossy(crate::util::trim_nul(name));
    std::env::var(n).ok().map(|v| v.encode_utf16().collect())
}

pub unsafe extern "system" fn GetEnvironmentVariableW(n: PCWSTR, buf: PWSTR, sz: u32) -> u32 {
    match env_w(&wstr(n)) {
        Some(v) => put_w(&v, buf, sz),
        None => 0,
    }
}

/// Expands %NAME% like Windows, and $NAME / ${NAME} and a leading ~ like a shell.
pub unsafe extern "system" fn ExpandEnvironmentStringsW(src: PCWSTR, dst: PWSTR, n: u32) -> u32 {
    let s = wstring(src);
    let mut out = String::new();
    let mut rest = s.as_str();
    if let Some(r) = rest.strip_prefix('~') {
        if r.is_empty() || r.starts_with('/') {
            out.push_str(&std::env::var("HOME").unwrap_or_default());
            rest = r;
        }
    }
    while let Some(i) = rest.find(['%', '$']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let (name, used) = if let Some(t) = tail.strip_prefix('%') {
            match t.find('%') {
                Some(j) => (&t[..j], j + 2),
                None => ("", 0),
            }
        } else if let Some(t) = tail.strip_prefix("${") {
            match t.find('}') {
                Some(j) => (&t[..j], j + 3),
                None => ("", 0),
            }
        } else {
            let t = &tail[1..];
            let j = t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(t.len());
            (&t[..j], if j == 0 { 0 } else { j + 1 })
        };
        match (used, std::env::var(name)) {
            (0, _) => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
            }
            (u, Ok(v)) => {
                out.push_str(&v);
                rest = &tail[u..];
            }
            (u, Err(_)) => {
                out.push_str(&tail[..u]);
                rest = &tail[u..];
            }
        }
    }
    out.push_str(rest);
    let w: Vec<u16> = out.encode_utf16().collect();
    if (w.len() + 1) as u32 > n {
        return (w.len() + 1) as u32;
    }
    put_w(&w, dst, n);
    (w.len() + 1) as u32
}

pub unsafe extern "system" fn GetModuleFileNameW(_m: HINSTANCE, buf: PWSTR, n: u32) -> u32 {
    let p = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let w: Vec<u16> = p.encode_utf16().collect();
    let r = put_w(&w, buf, n);
    if r > w.len() as u32 { 0 } else { r }
}

pub unsafe extern "system" fn GetModuleHandleW(_n: PCWSTR) -> HINSTANCE {
    1
}

pub unsafe extern "system" fn GetComputerNameW(b: PWSTR, n: *mut u32) -> BOOL {
    let mut h = [0u8; 256];
    let name = if libc::gethostname(h.as_mut_ptr() as *mut libc::c_char, h.len()) == 0 {
        let l = h.iter().position(|&c| c == 0).unwrap_or(h.len());
        String::from_utf8_lossy(&h[..l]).into_owned()
    } else {
        String::from("linux")
    };
    let w: Vec<u16> = name.encode_utf16().collect();
    let r = put_w(&w, b, *n);
    if r > w.len() as u32 {
        return 0;
    }
    *n = r;
    1
}

pub unsafe extern "system" fn ExitProcess(code: u32) -> ! {
    std::process::exit(code as i32)
}

// --------------------------------------------------------------------- time
pub unsafe extern "system" fn QueryPerformanceFrequency(f: *mut i64) -> BOOL {
    *f = 1_000_000_000;
    1
}

pub unsafe extern "system" fn QueryPerformanceCounter(c: *mut i64) -> BOOL {
    let mut ts: libc::timespec = core::mem::zeroed();
    libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    *c = ts.tv_sec * 1_000_000_000 + ts.tv_nsec;
    1
}

pub unsafe extern "system" fn GetTickCount() -> u32 {
    let mut c = 0;
    QueryPerformanceCounter(&mut c);
    (c / 1_000_000) as u32
}

pub unsafe extern "system" fn Sleep(ms: u32) {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

// ------------------------------------------------------------------- memory
pub unsafe extern "system" fn GetProcessHeap() -> HANDLE {
    1
}

pub unsafe extern "system" fn GetCurrentProcess() -> HANDLE {
    -1
}

/// Gives freed memory back to the system, like trimming the working set.
pub unsafe extern "system" fn HeapCompact(_h: HANDLE, _f: u32) -> usize {
    libc::malloc_trim(0);
    0
}

/// Like Windows' EmptyWorkingSet: drops what can be rebuilt, gives freed
/// heap back, then lets the kernel reclaim the process' pages. They come back
/// on their own the next time they are touched.
pub unsafe extern "system" fn K32EmptyWorkingSet(_p: HANDLE) -> BOOL {
    for f in TRIM_HOOKS {
        f();
    }
    libc::malloc_trim(0);
    let dbg = std::env::var_os("CROW_DEBUG_TRIM").is_some();
    let rss = || {
        let sm = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
        let mut out = String::new();
        let mut name = String::new();
        for l in sm.lines() {
            if l.split_whitespace().next().is_some_and(|f| f.contains('-') && f.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')) {
                name = l.split_whitespace().skip(1).take(1).chain(l.split_whitespace().skip(5).take(1)).collect::<Vec<_>>().join(" ");
            } else if let Some(v) = l.strip_prefix("Rss:") {
                if name.contains("CrowFO") || name.contains("libc") {
                    out.push_str(&format!("[{} {}] ", name.rsplit('/').next().unwrap_or(""), v.trim()));
                }
            }
        }
        out
    };
    if dbg {
        eprintln!("trim: before {}", rss());
    }
    page_out();
    if dbg {
        eprintln!("trim: after  {} live heap {} B", rss(), crate::LIVE.load(std::sync::atomic::Ordering::Relaxed));
    }
    1
}

/// Caches that are cheap to rebuild and freed when Crow goes idle.
const TRIM_HOOKS: &[fn()] = &[crate::icons::release, super::win::release_caches];

const MADV_COLD: i32 = 20;

/// Program and library code that was never modified is dropped from the
/// process (it stays in the system's file cache and comes back on its own),
/// and the heap is marked as the first thing to reclaim under memory pressure.
/// Modified pages, like relocated data, are left alone.
fn page_out() {
    let Ok(smaps) = std::fs::read_to_string("/proc/self/smaps") else { return };
    let mut cur: Option<(usize, usize, bool, bool)> = None; // start, end, file-backed read-only, anonymous
    let mut dirty = 0usize;
    let flush = |c: Option<(usize, usize, bool, bool)>, dirty: usize| {
        let Some((a, b, ro_file, anon)) = c else { return };
        let advice = if ro_file && dirty == 0 {
            libc::MADV_DONTNEED
        } else if anon {
            MADV_COLD
        } else {
            return;
        };
        unsafe { libc::madvise(a as *mut libc::c_void, b - a, advice) };
    };
    for l in smaps.lines() {
        let first = l.split_whitespace().next().unwrap_or("");
        if let Some((a, b)) = first.split_once('-').filter(|(a, _)| a.bytes().all(|c| c.is_ascii_hexdigit())) {
            flush(cur.take(), dirty);
            dirty = 0;
            let mut it = l.split_whitespace();
            let perms = it.nth(1).unwrap_or("");
            let name = l.split_whitespace().nth(5).unwrap_or("");
            let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16)) else { continue };
            let private = perms.ends_with('p');
            let ro_file = private && name.starts_with('/') && !perms.contains('w');
            let anon = private && perms.contains('w') && (name.is_empty() || name == "[heap]");
            cur = Some((a, b, ro_file, anon));
        } else if let Some(v) = l.strip_prefix("Private_Dirty:") {
            dirty += v.trim().trim_end_matches("kB").trim().parse::<usize>().unwrap_or(1);
        }
    }
    flush(cur, dirty);
}

/// Memory only this process uses (what Windows calls the private working set).
pub unsafe extern "system" fn K32GetProcessMemoryInfo(_p: HANDLE, c: *mut PROCESS_MEMORY_COUNTERS, _cb: u32) -> BOOL {
    let Ok(s) = std::fs::read_to_string("/proc/self/smaps_rollup") else { return 0 };
    let kb: usize = s
        .lines()
        .filter(|l| l.starts_with("Private_Clean:") || l.starts_with("Private_Dirty:"))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse::<usize>().ok())
        .sum();
    (*c).WorkingSetSize = kb * 1024;
    1
}

pub unsafe extern "system" fn GlobalAlloc(_f: u32, n: usize) -> isize {
    let k = k();
    k.next += 4;
    k.globals.insert(k.next, vec![0u8; n.max(1)]);
    k.next
}

pub unsafe extern "system" fn GlobalFree(h: isize) -> isize {
    k().globals.remove(&h);
    0
}

pub unsafe extern "system" fn GlobalLock(h: isize) -> *mut c_void {
    k().globals.get_mut(&h).map_or(core::ptr::null_mut(), |v| v.as_mut_ptr() as *mut c_void)
}

pub unsafe extern "system" fn GlobalUnlock(_h: isize) -> BOOL {
    1
}

pub(crate) fn global_take(h: isize) -> Option<Vec<u8>> {
    k().globals.remove(&h)
}

pub(crate) fn global_put(data: Vec<u8>) -> isize {
    let k = k();
    k.next += 4;
    k.globals.insert(k.next, data);
    k.next
}
