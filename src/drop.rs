//! Static, allocation-free OLE IDropTarget so files dragged from Explorer can
//! be dropped on (and hovered over) our folders.

use crate::ffi::*;
use crate::util::*;
use alloc::vec::Vec;
use core::ffi::c_void;

#[repr(C)]
struct Vtbl {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    enter: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINT, *mut u32) -> HRESULT,
    over: unsafe extern "system" fn(*mut c_void, u32, POINT, *mut u32) -> HRESULT,
    leave: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    drop: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, POINT, *mut u32) -> HRESULT,
}

#[repr(C)]
pub struct DropTarget {
    vtbl: *const Vtbl,
}
unsafe impl Sync for DropTarget {}

static VTBL: Vtbl = Vtbl { qi, add_ref, release, enter, over, leave, drop };
pub static TARGET: DropTarget = DropTarget { vtbl: &VTBL };

pub fn target_ptr() -> *mut c_void {
    &TARGET as *const DropTarget as *mut c_void
}

const IID_UNKNOWN: GUID = guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_DROP_TARGET: GUID = guid(0x122, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

unsafe extern "system" fn qi(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if *iid == IID_UNKNOWN || *iid == IID_DROP_TARGET {
        *out = this;
        S_OK
    } else {
        *out = core::ptr::null_mut();
        E_NOINTERFACE
    }
}
unsafe extern "system" fn add_ref(_: *mut c_void) -> u32 {
    2
}
unsafe extern "system" fn release(_: *mut c_void) -> u32 {
    1
}

/// Extracts file paths from an IDataObject (CF_HDROP).
unsafe fn files(data: *mut c_void) -> Vec<WString> {
    let mut out = Vec::new();
    if data.is_null() {
        return out;
    }
    let fe = FORMATETC { cfFormat: CF_HDROP, ptd: core::ptr::null_mut(), dwAspect: DVASPECT_CONTENT, lindex: -1, tymed: TYMED_HGLOBAL };
    let mut sm = STGMEDIUM { tymed: 0, data: 0, pUnkForRelease: core::ptr::null_mut() };
    let get: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> HRESULT = core::mem::transmute(vt(data, 3));
    if get(data, &fe, &mut sm) < 0 {
        return out;
    }
    let n = DragQueryFileW(sm.data, 0xFFFF_FFFF, core::ptr::null_mut(), 0);
    for i in 0..n.min(512) {
        let mut buf = [0u16; 1024];
        let l = DragQueryFileW(sm.data, i, buf.as_mut_ptr(), 1024) as usize;
        if l > 0 {
            out.push(buf[..l].to_vec());
        }
    }
    ReleaseStgMedium(&mut sm);
    out
}

// Windows' drag image helper. Telling it about enter, over, leave and drop lets
// Explorer's drag image render over our windows and always go away on drop.
const CLSID_DRAG_HELPER: GUID = guid(0x4657278A, 0x411B, 0x11D2, [0x83, 0x9A, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0xD0]);
const IID_DROP_HELPER: GUID = guid(0x4657278B, 0x411B, 0x11D2, [0x83, 0x9A, 0x00, 0xC0, 0x4F, 0xD9, 0x18, 0xD0]);
static HELPER: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

fn helper() -> *mut c_void {
    let h = HELPER.load(core::sync::atomic::Ordering::Relaxed);
    if h != 0 {
        return h as *mut c_void;
    }
    let mut p: *mut c_void = core::ptr::null_mut();
    unsafe {
        if CoCreateInstance(&CLSID_DRAG_HELPER, core::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_DROP_HELPER, &mut p) < 0 {
            return core::ptr::null_mut();
        }
    }
    HELPER.store(p as usize, core::sync::atomic::Ordering::Relaxed);
    p
}

unsafe extern "system" fn enter(_: *mut c_void, data: *mut c_void, _k: u32, pt: POINT, eff: *mut u32) -> HRESULT {
    let f = files(data);
    let a = crate::app::app();
    *eff = a.ole_enter(f, pt, *eff);
    let h = helper();
    if !h.is_null() {
        let target = a.wins.first().map_or(a.msg, |w| w.hwnd);
        let call: unsafe extern "system" fn(*mut c_void, HWND, *mut c_void, *const POINT, u32) -> HRESULT = core::mem::transmute(vt(h, 3));
        call(h, target, data, &pt, *eff);
    }
    S_OK
}
unsafe extern "system" fn over(_: *mut c_void, _k: u32, pt: POINT, eff: *mut u32) -> HRESULT {
    *eff = crate::app::app().ole_over(pt, *eff);
    let h = helper();
    if !h.is_null() {
        let call: unsafe extern "system" fn(*mut c_void, *const POINT, u32) -> HRESULT = core::mem::transmute(vt(h, 5));
        call(h, &pt, *eff);
    }
    S_OK
}
unsafe extern "system" fn leave(_: *mut c_void) -> HRESULT {
    crate::app::app().ole_leave();
    let h = helper();
    if !h.is_null() {
        let call: unsafe extern "system" fn(*mut c_void) -> HRESULT = core::mem::transmute(vt(h, 4));
        call(h);
    }
    S_OK
}
unsafe extern "system" fn drop(_: *mut c_void, data: *mut c_void, _k: u32, pt: POINT, eff: *mut u32) -> HRESULT {
    let h = helper();
    if !h.is_null() {
        let call: unsafe extern "system" fn(*mut c_void, *mut c_void, *const POINT, u32) -> HRESULT = core::mem::transmute(vt(h, 6));
        call(h, data, &pt, *eff);
    }
    *eff = crate::app::app().ole_drop(pt);
    S_OK
}
