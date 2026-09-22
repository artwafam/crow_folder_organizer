//! crow_menu.dll adds Crow to the Windows 11 desktop context menu.
//!
//! It implements IExplorerCommand with a "Crow" submenu holding "Nova pasta"
//! and "Novo painel". All COM objects are static, with no heap, no std and no
//! CRT. Explorer loads this DLL and the Crow process never does. Running a
//! command starts `crow.exe --new` or `crow.exe --new-panel`, which hands the
//! request to the running instance.
#![no_std]
#![allow(non_snake_case, clippy::missing_safety_doc)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

type HRESULT = i32;
const S_OK: HRESULT = 0;
const S_FALSE: HRESULT = 1;
const E_NOTIMPL: HRESULT = 0x8000_4001u32 as i32;
const E_NOINTERFACE: HRESULT = 0x8000_4002u32 as i32;
const E_POINTER: HRESULT = 0x8000_4003u32 as i32;
const E_OUTOFMEMORY: HRESULT = 0x8007_000Eu32 as i32;
const CLASS_E_CLASSNOTAVAILABLE: HRESULT = 0x8004_0111u32 as i32;
const CLASS_E_NOAGGREGATION: HRESULT = 0x8004_0110u32 as i32;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GUID {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}
const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> GUID {
    GUID { d1, d2, d3, d4 }
}

/// Must match the Clsid in AppxManifest.xml.
pub const CLSID_CROW_MENU: GUID = guid(0x7A3F5C21, 0x9B4E, 0x4D8A, [0xA1, 0xC6, 0x3E, 0x2B, 0x8F, 0x4D, 0x9C, 0x17]);
const IID_IUNKNOWN: GUID = guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_ICLASSFACTORY: GUID = guid(1, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_IEXPLORERCOMMAND: GUID = guid(0xa08ce4d0, 0xfa25, 0x44ab, [0xb5, 0x7c, 0xc7, 0xb1, 0xc3, 0x23, 0xe0, 0xb9]);
const IID_IENUMEXPLORERCOMMAND: GUID = guid(0xa88826f8, 0x186f, 0x4987, [0xaa, 0xde, 0xea, 0x0c, 0xef, 0x8f, 0xbf, 0xe8]);

#[repr(C)]
struct StartupInfo {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    rest: [u32; 8],
    flags: u32,
    show: u16,
    cb2: u16,
    r2: *mut u8,
    h_in: isize,
    h_out: isize,
    h_err: isize,
}
#[repr(C)]
struct ProcessInfo {
    process: isize,
    thread: isize,
    pid: u32,
    tid: u32,
}

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn GetModuleFileNameW(m: isize, buf: *mut u16, n: u32) -> u32;
    fn CreateProcessW(app: *const u16, cmd: *mut u16, pa: *const c_void, ta: *const c_void, inherit: i32, flags: u32,
                      env: *const c_void, dir: *const u16, si: *const StartupInfo, pi: *mut ProcessInfo) -> i32;
    fn CloseHandle(h: isize) -> i32;
}
#[link(name = "ole32", kind = "raw-dylib")]
extern "system" {
    fn CoTaskMemAlloc(n: usize) -> *mut c_void;
}

static MODULE: AtomicIsize = AtomicIsize::new(0);

#[no_mangle]
pub extern "system" fn DllMain(h: isize, reason: u32, _r: *mut c_void) -> i32 {
    if reason == 1 {
        MODULE.store(h, Ordering::Relaxed);
    }
    1
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if *clsid != CLSID_CROW_MENU {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    factory_qi(obj(&FACTORY), iid, out)
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE // the objects are static, so the DLL stays loaded
}

// ------------------------------------------------------------ static objects
#[repr(C)]
struct Obj<V: 'static> {
    vtbl: &'static V,
    kind: u32,
}
unsafe impl<V> Sync for Obj<V> {}

fn obj<V>(o: &'static Obj<V>) -> *mut c_void {
    o as *const Obj<V> as *mut c_void
}
unsafe fn kind(this: *mut c_void) -> u32 {
    (*(this as *const Obj<()>)).kind
}

unsafe extern "system" fn add_ref(_: *mut c_void) -> u32 {
    2
}
unsafe extern "system" fn release(_: *mut c_void) -> u32 {
    1
}

// IClassFactory
#[repr(C)]
struct FactoryVtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    create: unsafe extern "system" fn(*mut c_void, *mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    lock: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
}
static FACTORY_VTBL: FactoryVtbl = FactoryVtbl { qi: factory_qi, add_ref, release, create: factory_create, lock: factory_lock };
static FACTORY: Obj<FactoryVtbl> = Obj { vtbl: &FACTORY_VTBL, kind: 0 };

unsafe extern "system" fn factory_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if *iid == IID_IUNKNOWN || *iid == IID_ICLASSFACTORY {
        *out = this;
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}
unsafe extern "system" fn factory_create(_: *mut c_void, outer: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    cmd_qi(obj(&ROOT), iid, out)
}
unsafe extern "system" fn factory_lock(_: *mut c_void, _: i32) -> HRESULT {
    S_OK
}

// IExplorerCommand
#[repr(C)]
struct CmdVtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_title: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut u16) -> HRESULT,
    get_icon: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut u16) -> HRESULT,
    get_tooltip: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut u16) -> HRESULT,
    get_canonical_name: unsafe extern "system" fn(*mut c_void, *mut GUID) -> HRESULT,
    get_state: unsafe extern "system" fn(*mut c_void, *mut c_void, i32, *mut u32) -> HRESULT,
    invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
    get_flags: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    enum_sub: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}
static CMD_VTBL: CmdVtbl = CmdVtbl {
    qi: cmd_qi,
    add_ref,
    release,
    get_title,
    get_icon,
    get_tooltip,
    get_canonical_name,
    get_state,
    invoke,
    get_flags,
    enum_sub,
};
const K_ROOT: u32 = 0;
const K_NEW: u32 = 1;
const K_PANEL: u32 = 2;
static ROOT: Obj<CmdVtbl> = Obj { vtbl: &CMD_VTBL, kind: K_ROOT };
static NEW: Obj<CmdVtbl> = Obj { vtbl: &CMD_VTBL, kind: K_NEW };
static PANEL: Obj<CmdVtbl> = Obj { vtbl: &CMD_VTBL, kind: K_PANEL };

unsafe extern "system" fn cmd_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_IUNKNOWN || *iid == IID_IEXPLORERCOMMAND {
        *out = this;
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}

/// Copies a UTF-16 string into CoTaskMem (caller frees).
unsafe fn dup(s: &[u16], out: *mut *mut u16) -> HRESULT {
    let p = CoTaskMemAlloc((s.len() + 1) * 2) as *mut u16;
    if p.is_null() {
        return E_OUTOFMEMORY;
    }
    for (i, &c) in s.iter().enumerate() {
        *p.add(i) = c;
    }
    *p.add(s.len()) = 0;
    *out = p;
    S_OK
}

/// Turns an ASCII or Latin-1 literal into UTF-16 at compile time.
macro_rules! wide {
    ($s:literal) => {{
        const S: &str = $s;
        const N: usize = S.len();
        const W: [u16; N] = {
            let b = S.as_bytes();
            let mut o = [0u16; N];
            let mut i = 0;
            while i < N {
                o[i] = b[i] as u16;
                i += 1;
            }
            o
        };
        &W
    }};
}

unsafe extern "system" fn get_title(this: *mut c_void, _: *mut c_void, out: *mut *mut u16) -> HRESULT {
    match kind(this) {
        K_ROOT => dup(wide!("Crow"), out),
        K_NEW => dup(wide!("Nova pasta"), out),
        _ => dup(wide!("Novo painel"), out),
    }
}

/// Directory of this DLL (where crow.exe and crow.ico live), with trailing '\'.
unsafe fn module_dir(buf: &mut [u16; 520]) -> usize {
    let n = GetModuleFileNameW(MODULE.load(Ordering::Relaxed), buf.as_mut_ptr(), 520) as usize;
    let mut i = n.min(519);
    while i > 0 && buf[i - 1] != b'\\' as u16 {
        i -= 1;
    }
    i
}

unsafe extern "system" fn get_icon(this: *mut c_void, _: *mut c_void, out: *mut *mut u16) -> HRESULT {
    if kind(this) != K_ROOT {
        return E_NOTIMPL;
    }
    let mut buf = [0u16; 520];
    let n = module_dir(&mut buf);
    let name = wide!("crow.ico");
    if n + name.len() >= 520 {
        return E_NOTIMPL;
    }
    buf[n..n + name.len()].copy_from_slice(name);
    dup(&buf[..n + name.len()], out)
}
unsafe extern "system" fn get_tooltip(_: *mut c_void, _: *mut c_void, out: *mut *mut u16) -> HRESULT {
    *out = core::ptr::null_mut();
    E_NOTIMPL
}
unsafe extern "system" fn get_canonical_name(this: *mut c_void, g: *mut GUID) -> HRESULT {
    let mut v = CLSID_CROW_MENU;
    v.d4[7] = v.d4[7].wrapping_add(kind(this) as u8);
    *g = v;
    S_OK
}
unsafe extern "system" fn get_state(_: *mut c_void, _: *mut c_void, _slow: i32, state: *mut u32) -> HRESULT {
    *state = 0; // ECS_ENABLED
    S_OK
}
unsafe extern "system" fn get_flags(this: *mut c_void, flags: *mut u32) -> HRESULT {
    *flags = if kind(this) == K_ROOT { 1 /*ECF_HASSUBCOMMANDS*/ } else { 0 };
    S_OK
}

unsafe extern "system" fn invoke(this: *mut c_void, _: *mut c_void, _: *mut c_void) -> HRESULT {
    let arg: &[u16] = match kind(this) {
        K_NEW => wide!(" --new"),
        K_PANEL => wide!(" --new-panel"),
        _ => return S_OK,
    };
    let mut exe = [0u16; 520];
    let n = module_dir(&mut exe);
    let name = wide!("crow.exe");
    if n + name.len() + 1 >= 520 {
        return S_OK;
    }
    exe[n..n + name.len()].copy_from_slice(name);
    let elen = n + name.len();
    exe[elen] = 0;
    // the command line is "<exe>" --new
    let mut cmd = [0u16; 600];
    let mut k = 0;
    cmd[k] = b'"' as u16;
    k += 1;
    cmd[k..k + elen].copy_from_slice(&exe[..elen]);
    k += elen;
    cmd[k] = b'"' as u16;
    k += 1;
    cmd[k..k + arg.len()].copy_from_slice(arg);
    k += arg.len();
    cmd[k] = 0;
    let mut si: StartupInfo = core::mem::zeroed();
    si.cb = core::mem::size_of::<StartupInfo>() as u32;
    let mut pi: ProcessInfo = core::mem::zeroed();
    if CreateProcessW(exe.as_ptr(), cmd.as_mut_ptr(), core::ptr::null(), core::ptr::null(), 0, 0,
        core::ptr::null(), core::ptr::null(), &si, &mut pi) != 0
    {
        CloseHandle(pi.thread);
        CloseHandle(pi.process);
    }
    S_OK
}

// IEnumExplorerCommand over [NEW, PANEL]
#[repr(C)]
struct EnumVtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    next: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> HRESULT,
    skip: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    reset: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    clone: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}
static ENUM_VTBL: EnumVtbl = EnumVtbl { qi: enum_qi, add_ref, release, next: enum_next, skip: enum_skip, reset: enum_reset, clone: enum_clone };
static ENUM: Obj<EnumVtbl> = Obj { vtbl: &ENUM_VTBL, kind: 0 };
static POS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn enum_sub(this: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    if kind(this) != K_ROOT {
        *out = core::ptr::null_mut();
        return E_NOTIMPL;
    }
    POS.store(0, Ordering::Relaxed);
    *out = obj(&ENUM);
    S_OK
}
unsafe extern "system" fn enum_qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if *iid == IID_IUNKNOWN || *iid == IID_IENUMEXPLORERCOMMAND {
        *out = this;
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}
unsafe extern "system" fn enum_next(_: *mut c_void, celt: u32, out: *mut *mut c_void, fetched: *mut u32) -> HRESULT {
    let items = [obj(&NEW), obj(&PANEL)];
    let mut n = 0u32;
    while n < celt {
        let p = POS.load(Ordering::Relaxed) as usize;
        if p >= items.len() {
            break;
        }
        *out.add(n as usize) = items[p];
        POS.store(p as u32 + 1, Ordering::Relaxed);
        n += 1;
    }
    if !fetched.is_null() {
        *fetched = n;
    }
    if n == celt { S_OK } else { S_FALSE }
}
unsafe extern "system" fn enum_skip(_: *mut c_void, celt: u32) -> HRESULT {
    POS.fetch_add(celt, Ordering::Relaxed);
    S_OK
}
unsafe extern "system" fn enum_reset(_: *mut c_void) -> HRESULT {
    POS.store(0, Ordering::Relaxed);
    S_OK
}
unsafe extern "system" fn enum_clone(_: *mut c_void, out: *mut *mut c_void) -> HRESULT {
    *out = core::ptr::null_mut();
    E_NOTIMPL
}

// ------------------------------------------------------------- runtime bits
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    core::arch::asm!("rep movsb", inout("rcx") n => _, inout("rdi") d => _, inout("rsi") s => _, options(nostack, preserves_flags));
    d
}
#[no_mangle]
pub unsafe extern "C" fn memset(d: *mut c_void, c: i32, n: usize) -> *mut c_void {
    core::arch::asm!("rep stosb", inout("rcx") n => _, inout("rdi") d => _, in("al") c as u8, options(nostack, preserves_flags));
    d
}
#[no_mangle]
pub unsafe extern "C" fn memmove(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    let (dd, ss) = (d as *mut u8, s as *const u8);
    if (dd as usize).wrapping_sub(ss as usize) >= n {
        memcpy(d, s, n)
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            core::ptr::write_volatile(dd.add(i), core::ptr::read_volatile(ss.add(i)));
        }
        d
    }
}
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a as *const u8, b as *const u8);
    for i in 0..n {
        let (x, y) = (core::ptr::read_volatile(a.add(i)), core::ptr::read_volatile(b.add(i)));
        if x != y {
            return x as i32 - y as i32;
        }
    }
    0
}
#[no_mangle]
pub static _fltused: i32 = 0;
