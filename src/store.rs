//! Disk layout. Data that should follow the user lives inside the Desktop
//! folder, so anything that backs up or syncs the Desktop (OneDrive, or
//! restoring a new PC) carries Crow FO along. Moving a shortcut into a folder is
//! then just a move inside the synced tree.
//!
//!   <Desktop>\.crowfo\crow.ini        settings and per-PC positions
//!   <Desktop>\.crowfo\f<id>\*.lnk     the shortcuts of folder <id>
//!
//! Data that belongs to this machine and can be rebuilt stays local.
//!
//!   %LOCALAPPDATA%\Crow FO\cache\f<id>.icons   memory-mapped icon cache

use crate::ffi::*;
use crate::util::*;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::null_mut;

static mut ROOT: Option<WString> = None;

/// <Desktop>\.crowfo (hidden), resolved once per run.
pub fn root() -> WString {
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(ROOT);
        if let Some(r) = slot {
            return r.clone();
        }
        // CROW_DATA overrides the location (portable installs, testing)
        let custom = env(w!("CROW_DATA"));
        let r = if custom.is_empty() { wcat(&[&desktop_dir(), w!("\\.crowfo")]) } else { custom };
        mkdir(&r);
        let p = wz(&r);
        let a = GetFileAttributesW(p.as_ptr());
        if a != INVALID_FILE_ATTRIBUTES && a & FILE_ATTRIBUTE_HIDDEN == 0 {
            SetFileAttributesW(p.as_ptr(), a | FILE_ATTRIBUTE_HIDDEN);
        }
        *slot = Some(r.clone());
        r
    }
}

/// %LOCALAPPDATA%\Crow FO\cache, local to this machine and never synced.
pub fn cache_dir() -> WString {
    let mut base = env(w!("LOCALAPPDATA"));
    if base.is_empty() {
        base = wcat(&[&env(w!("USERPROFILE")), w!("\\AppData\\Local")]);
    }
    let c = wcat(&[&base, w!("\\Crow FO")]);
    mkdir(&c);
    let d = wcat(&[&c, w!("\\cache")]);
    mkdir(&d);
    d
}

pub fn computer_name() -> alloc::string::String {
    let mut buf = [0u16; 64];
    let mut n = 64u32;
    unsafe { GetComputerNameW(buf.as_mut_ptr(), &mut n) };
    from_w(&buf[..(n as usize).min(64)])
}
pub fn ini_path() -> WString {
    wcat(&[&root(), w!("\\crow.ini")])
}
pub fn folder_dir(id: u32) -> WString {
    let mut s = alloc::string::String::from("\\f");
    fmt_int(id as i64, &mut s);
    let d = wcat(&[&root(), &to_w(&s)]);
    mkdir(&d);
    d
}

pub fn desktop_dir() -> WString {
    unsafe {
        let mut k: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders").as_ptr(),
            0, KEY_READ, &mut k) == 0
        {
            let mut buf = [0u16; 520];
            let mut len = (buf.len() * 2) as u32;
            let mut ty = 0u32;
            let r = RegQueryValueExW(k, w!("Desktop").as_ptr(), null_mut(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut len);
            RegCloseKey(k);
            if r == 0 {
                let v = expand_env(&buf);
                if is_dir(&v) {
                    return v;
                }
            }
        }
    }
    wcat(&[&env(w!("USERPROFILE")), w!("\\Desktop")])
}

/// Builds `dir\name`, adding " (2)", " (3)" and so on before the extension when the name is taken.
pub fn unique_path(dir: &[u16], name: &[u16]) -> WString {
    let first = wcat(&[dir, w!("\\"), name]);
    if !exists(&first) {
        return first;
    }
    let st = stem(name);
    let ext = &name[st.len()..];
    for i in 2..1000 {
        let mut n = alloc::string::String::from(" (");
        fmt_int(i, &mut n);
        n.push(')');
        let p = wcat(&[dir, w!("\\"), st, &to_w(&n), ext]);
        if !exists(&p) {
            return p;
        }
    }
    first
}

pub fn is_shortcut(p: &[u16]) -> bool {
    ext_is(p, "lnk") || ext_is(p, "url") || ext_is(p, "pif") || ext_is(p, "appref-ms")
}

pub enum Import {
    Done(WString),
    /// The shortcut sits on the shared desktop and we may not move it yet.
    NeedsAdmin,
    Failed,
}

/// The desktop shared by all users (C:\Users\Public\Desktop).
fn public_desktop() -> WString {
    let p = env(w!("PUBLIC"));
    if p.is_empty() {
        return Vec::new();
    }
    wcat(&[&p, w!("\\Desktop")])
}

/// Brings `src` into folder `dir` and returns the new file name. Shortcuts are
/// moved. Anything else gets a new shortcut pointing at it, so the user's real
/// files never move.
pub fn import(dir: &[u16], src: &[u16]) -> Import {
    let src = trim_nul(src);
    if weq_ic(parent_dir(src), dir) {
        return Import::Done(file_name(src).to_vec());
    }
    unsafe {
        if is_shortcut(src) {
            let dst = unique_path(dir, file_name(src));
            let (s, d) = (wz(src), wz(&dst));
            if MoveFileExW(s.as_ptr(), d.as_ptr(), MOVEFILE_COPY_ALLOWED) != 0 {
                return Import::Done(file_name(&dst).to_vec());
            }
            let pd = public_desktop();
            if GetLastError() == ERROR_ACCESS_DENIED && !pd.is_empty() && weq_ic(parent_dir(src), &pd) {
                return Import::NeedsAdmin;
            }
            return match copy_in(dir, src) {
                Some(n) => Import::Done(n),
                None => Import::Failed,
            };
        }
        let name = if is_dir(src) { file_name(src).to_vec() } else { stem(src).to_vec() };
        let name = if name.is_empty() { src.iter().take(1).copied().collect() } else { name };
        let dst = unique_path(dir, &wcat(&[&name, w!(".lnk")]));
        if create_link(&dst, src) {
            Import::Done(file_name(&dst).to_vec())
        } else {
            Import::Failed
        }
    }
}

/// Copies a shortcut in, leaving the original where it is.
pub fn copy_in(dir: &[u16], src: &[u16]) -> Option<WString> {
    let dst = unique_path(dir, file_name(src));
    unsafe {
        if CopyFileW(wz(src).as_ptr(), wz(&dst).as_ptr(), 1) != 0 {
            Some(file_name(&dst).to_vec())
        } else {
            None
        }
    }
}


const CLSID_SHELL_LINK: GUID = guid(0x00021401, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_SHELL_LINK_W: GUID = guid(0x000214F9, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_PERSIST_FILE: GUID = guid(0x0000010b, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

pub unsafe fn create_link(lnk: &[u16], target: &[u16]) -> bool {
    let mut sl: *mut c_void = null_mut();
    if CoCreateInstance(&CLSID_SHELL_LINK, null_mut(), CLSCTX_INPROC_SERVER, &IID_SHELL_LINK_W, &mut sl) < 0 {
        return false;
    }
    let t = wz(target);
    let set_path: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT = core::mem::transmute(vt(sl, 20));
    let set_wd: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT = core::mem::transmute(vt(sl, 9));
    set_path(sl, t.as_ptr());
    let wd = wz(parent_dir(target));
    set_wd(sl, wd.as_ptr());
    let mut pf: *mut c_void = null_mut();
    let qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT = core::mem::transmute(vt(sl, 0));
    let mut ok = false;
    if qi(sl, &IID_PERSIST_FILE, &mut pf) >= 0 {
        let save: unsafe extern "system" fn(*mut c_void, PCWSTR, BOOL) -> HRESULT = core::mem::transmute(vt(pf, 6));
        let l = wz(lnk);
        ok = save(pf, l.as_ptr(), 1) >= 0;
        com_release(pf);
    }
    com_release(sl);
    ok
}

/// Moves a shortcut out of the organizer back to the desktop.
pub fn export_to_desktop(path: &[u16]) -> bool {
    let dst = unique_path(&desktop_dir(), file_name(path));
    unsafe { MoveFileExW(wz(path).as_ptr(), wz(&dst).as_ptr(), MOVEFILE_COPY_ALLOWED) != 0 }
}

pub fn move_between(src: &[u16], dir: &[u16]) -> Option<WString> {
    let dst = unique_path(dir, file_name(src));
    unsafe {
        if MoveFileExW(wz(src).as_ptr(), wz(&dst).as_ptr(), MOVEFILE_COPY_ALLOWED) != 0 {
            Some(file_name(&dst).to_vec())
        } else {
            None
        }
    }
}

pub fn rename(dir: &[u16], old: &[u16], new_stem: &[u16]) -> Option<WString> {
    let ext = &old[stem(old).len()..];
    let clean: WString = new_stem.iter().copied().filter(|&c| !b"\\/:*?\"<>|".contains(&(c as u8)) || c > 127).collect();
    if clean.is_empty() {
        return None;
    }
    let dst = unique_path(dir, &wcat(&[&clean, ext]));
    let src = wcat(&[dir, w!("\\"), old]);
    unsafe {
        if MoveFileExW(wz(&src).as_ptr(), wz(&dst).as_ptr(), 0) != 0 {
            Some(file_name(&dst).to_vec())
        } else {
            None
        }
    }
}

pub fn launch(path: &[u16]) {
    unsafe {
        let p = wz(path);
        let d = wz(parent_dir(path));
        ShellExecuteW(0, core::ptr::null(), p.as_ptr(), core::ptr::null(), d.as_ptr(), SW_SHOWNORMAL);
    }
}
pub fn reveal(path: &[u16]) {
    unsafe {
        let args = wz(&wcat(&[w!("/select,\""), path, w!("\"")]));
        ShellExecuteW(0, core::ptr::null(), w!("explorer.exe").as_ptr(), args.as_ptr(), core::ptr::null(), SW_SHOWNORMAL);
    }
}

// --------------------------------------------------------------- autostart
pub const RUN_KEY: &[u16] = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

pub fn own_exe() -> WString {
    let mut exe = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(0, exe.as_mut_ptr(), 520) } as usize;
    exe[..n].to_vec()
}

/// Turns "start with Windows" on or off for this CrowFO.exe.
pub fn set_autostart(on: bool) {
    set_autostart_for(&own_exe(), on);
}

/// Same, for a given CrowFO.exe (used by the installer). The app starts with
/// --background so it doesn't open its settings window at every sign-in.
pub fn set_autostart_for(exe: &[u16], on: bool) {
    unsafe {
        let mut k: HKEY = 0;
        if RegCreateKeyExW(HKEY_CURRENT_USER, RUN_KEY.as_ptr(), 0, core::ptr::null(), 0, KEY_WRITE | KEY_READ, core::ptr::null(), &mut k, null_mut()) != 0 {
            return;
        }
        if on {
            let v = wz(&wcat(&[w!("\""), exe, w!("\" --background")]));
            let mut cur = [0u16; 600];
            let mut len = (cur.len() * 2) as u32;
            let mut ty = 0;
            let same = RegQueryValueExW(k, w!("Crow FO").as_ptr(), null_mut(), &mut ty, cur.as_mut_ptr() as *mut u8, &mut len) == 0
                && trim_nul(&cur) == trim_nul(&v);
            if !same {
                RegSetValueExW(k, w!("Crow FO").as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
            }
        } else {
            RegDeleteValueW(k, w!("Crow FO").as_ptr());
        }
        RegCloseKey(k);
    }
}

// ------------------------------------------------- desktop context menu
// Classic verbs in HKCU, which need no admin rights. Windows 10 shows them
// directly and Windows 11 shows them under "Show more options". A signed build
// can also register the Windows 11 top level entry (crow_menu.dll).
pub const DESK_KEY: &[u16] = w!("Software\\Classes\\DesktopBackground\\Shell\\CrowFO");

pub fn reg_str(path: &[u16], name: Option<&[u16]>, val: &[u16]) {
    unsafe {
        let mut k: HKEY = 0;
        if RegCreateKeyExW(HKEY_CURRENT_USER, wz(path).as_ptr(), 0, core::ptr::null(), 0, KEY_WRITE, core::ptr::null(), &mut k, null_mut()) != 0 {
            return;
        }
        let v = wz(val);
        let n = name.map(wz);
        RegSetValueExW(k, n.as_ref().map_or(core::ptr::null(), |n| n.as_ptr()), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
        RegCloseKey(k);
    }
}

pub fn reg_dword(path: &[u16], name: &[u16], val: u32) {
    unsafe {
        let mut k: HKEY = 0;
        if RegCreateKeyExW(HKEY_CURRENT_USER, wz(path).as_ptr(), 0, core::ptr::null(), 0, KEY_WRITE, core::ptr::null(), &mut k, null_mut()) != 0 {
            return;
        }
        let n = wz(name);
        RegSetValueExW(k, n.as_ptr(), 0, REG_DWORD, &val as *const u32 as *const u8, 4);
        RegCloseKey(k);
    }
}

pub fn reg_delete_tree(path: &[u16]) {
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wz(path).as_ptr()) };
}

pub fn set_desktop_menu(on: bool) {
    set_desktop_menu_for(&own_exe(), on);
}

/// Registers (or removes) "Crow FO" in the desktop's right-click menu, with the
/// labels in the current language.
pub fn set_desktop_menu_for(exe: &[u16], on: bool) {
    reg_delete_tree(DESK_KEY);
    if !on {
        return;
    }
    let q = |arg: &[u16]| wcat(&[w!("\""), exe, w!("\""), arg]);
    let tr = |k: &'static str| to_w(crate::i18n::tr(k));
    reg_str(DESK_KEY, Some(w!("MUIVerb")), w!("Crow FO"));
    reg_str(DESK_KEY, Some(w!("Icon")), exe);
    reg_str(DESK_KEY, Some(w!("SubCommands")), w!(""));
    let a = wcat(&[DESK_KEY, w!("\\shell\\a_new")]);
    reg_str(&a, Some(w!("MUIVerb")), &tr("menu.new_folder"));
    reg_str(&wcat(&[&a, w!("\\command")]), None, &q(w!(" --new")));
    let b = wcat(&[DESK_KEY, w!("\\shell\\b_panel")]);
    reg_str(&b, Some(w!("MUIVerb")), &tr("menu.new_panel"));
    reg_str(&wcat(&[&b, w!("\\command")]), None, &q(w!(" --new-panel")));
    let c = wcat(&[DESK_KEY, w!("\\shell\\c_settings")]);
    reg_str(&c, Some(w!("MUIVerb")), &tr("menu.app_settings"));
    reg_str(&c, Some(w!("CommandFlags")), w!("32")); // separator above
    reg_str(&wcat(&[&c, w!("\\command")]), None, &q(w!(" --settings")));
}

/// Sends a file to the Recycle Bin.
pub fn recycle(path: &[u16]) -> bool {
    let mut from = wz(path);
    from.push(0); // the list must end with two zeros
    unsafe {
        let mut op: SHFILEOPSTRUCTW = core::mem::zeroed();
        op.wFunc = FO_DELETE;
        op.pFrom = from.as_ptr();
        op.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI;
        SHFileOperationW(&mut op) == 0 && !exists(path)
    }
}

// -------------------------------------------------------------- icon cache
/// Read-only memory-mapped icon cache for one folder. Pages are file-backed,
/// so they never count against the process' private memory and the OS can
/// drop them at will.
pub struct IconCache {
    map: HANDLE,
    view: *const u8,
    len: usize,
}

const MAGIC: u32 = 0x3157_5243; // "CRW1"

impl IconCache {
    pub const fn empty() -> IconCache {
        IconCache { map: 0, view: core::ptr::null(), len: 0 }
    }
    pub fn path(dir: &[u16]) -> WString {
        wcat(&[&cache_dir(), w!("\\"), file_name(dir), w!(".icons")])
    }
    pub fn open(dir: &[u16]) -> IconCache {
        unsafe {
            let p = wz(&Self::path(dir));
            let h = CreateFileW(p.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE, core::ptr::null(), OPEN_EXISTING, 0, 0);
            if h == INVALID_HANDLE_VALUE {
                return Self::empty();
            }
            let mut sz = 0i64;
            GetFileSizeEx(h, &mut sz);
            if sz < 8 {
                CloseHandle(h);
                return Self::empty();
            }
            let m = CreateFileMappingW(h, core::ptr::null(), PAGE_READONLY, 0, 0, core::ptr::null());
            CloseHandle(h);
            if m == 0 {
                return Self::empty();
            }
            let v = MapViewOfFile(m, FILE_MAP_READ, 0, 0, 0) as *const u8;
            if v.is_null() {
                CloseHandle(m);
                return Self::empty();
            }
            let c = IconCache { map: m, view: v, len: sz as usize };
            if c.u32_at(0) != MAGIC {
                return Self::empty();
            }
            c
        }
    }
    pub fn is_open(&self) -> bool {
        !self.view.is_null()
    }
    fn u32_at(&self, off: usize) -> u32 {
        if off + 4 > self.len {
            return 0;
        }
        unsafe { core::ptr::read_unaligned(self.view.add(off) as *const u32) }
    }
    /// Finds an icon of s by s pixels by its key.
    pub fn get(&self, key: u64, s: i32) -> Option<&[u32]> {
        if self.view.is_null() {
            return None;
        }
        let n = self.u32_at(4) as usize;
        for i in 0..n {
            let e = 8 + i * 16;
            let k = self.u32_at(e) as u64 | (self.u32_at(e + 4) as u64) << 32;
            if k == key && self.u32_at(e + 8) as i32 == s {
                let off = self.u32_at(e + 12) as usize;
                let cnt = (s * s) as usize;
                if off % 4 != 0 || off + cnt * 4 > self.len {
                    return None;
                }
                return Some(unsafe { core::slice::from_raw_parts(self.view.add(off) as *const u32, cnt) });
            }
        }
        None
    }
    pub fn close(&mut self) {
        unsafe {
            if !self.view.is_null() {
                UnmapViewOfFile(self.view as _);
                CloseHandle(self.map);
            }
        }
        self.view = core::ptr::null();
        self.map = 0;
        self.len = 0;
    }
}
impl Drop for IconCache {
    fn drop(&mut self) {
        self.close();
    }
}

/// Writes a new cache file with the given (key, size, pixels) entries.
pub fn write_cache(dir: &[u16], entries: &[(u64, i32, Vec<u32>)]) {
    let n = entries.len();
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    let mut off = 8 + n * 16;
    for (k, s, px) in entries {
        out.extend_from_slice(&k.to_le_bytes());
        out.extend_from_slice(&(*s as u32).to_le_bytes());
        out.extend_from_slice(&(off as u32).to_le_bytes());
        off += px.len() * 4;
    }
    for (_, _, px) in entries {
        for v in px {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    let p = IconCache::path(dir);
    // CREATE_ALWAYS refuses to overwrite hidden files, so remove it first.
    unsafe { DeleteFileW(wz(&p).as_ptr()) };
    write_file(&p, &out);
    unsafe { SetFileAttributesW(wz(&p).as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
}
