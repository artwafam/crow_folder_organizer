//! Minimal runtime without CRT or std. It provides an allocator on top of the
//! process heap, the panic handler and the few compiler intrinsics the MSVC
//! target expects.

use crate::ffi::*;
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

pub struct Heap;

unsafe impl GlobalAlloc for Heap {
    #[inline]
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // Process heap guarantees 16-byte alignment on x64.
        if l.align() <= 16 {
            HeapAlloc(GetProcessHeap(), 0, l.size()) as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if l.align() <= 16 {
            HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, l.size()) as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }
    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, _l: Layout) {
        HeapFree(GetProcessHeap(), 0, p as _);
    }
    #[inline]
    unsafe fn realloc(&self, p: *mut u8, _l: Layout, n: usize) -> *mut u8 {
        HeapReAlloc(GetProcessHeap(), 0, p as _, n) as *mut u8
    }
}

#[global_allocator]
static GLOBAL: Heap = Heap;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { ExitProcess(3) }
}

#[no_mangle]
#[used]
pub static _fltused: i32 = 0;

// ---- memory intrinsics, implemented with x86 string instructions ----

#[no_mangle]
pub unsafe extern "C" fn memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    core::arch::asm!("rep movsb", inout("rcx") n => _, inout("rdi") d => _, inout("rsi") s => _,
        options(nostack, preserves_flags));
    d
}

#[no_mangle]
pub unsafe extern "C" fn memmove(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (d as *mut u8, s as *const u8);
    if (d as usize).wrapping_sub(s as usize) >= n {
        // forward copy is safe
        core::arch::asm!("rep movsb", inout("rcx") n => _, inout("rdi") d => _, inout("rsi") s => _,
            options(nostack, preserves_flags));
    } else {
        core::arch::asm!("std", "rep movsb", "cld",
            inout("rcx") n => _, inout("rdi") d.add(n).wrapping_sub(1) => _,
            inout("rsi") s.add(n).wrapping_sub(1) => _, options(nostack));
    }
    d as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn memset(d: *mut c_void, c: i32, n: usize) -> *mut c_void {
    core::arch::asm!("rep stosb", inout("rcx") n => _, inout("rdi") d => _, in("al") c as u8,
        options(nostack, preserves_flags));
    d
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a as *const u8, b as *const u8);
    let mut i = 0;
    while i < n {
        let x = core::ptr::read_volatile(a.add(i));
        let y = core::ptr::read_volatile(b.add(i));
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    memcmp(a, b, n)
}

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const i8) -> usize {
    let s = s as *const u8;
    let mut n = 0;
    while core::ptr::read_volatile(s.add(n)) != 0 {
        n += 1;
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn wcslen(s: *const u16) -> usize {
    let mut n = 0;
    while core::ptr::read_volatile(s.add(n)) != 0 {
        n += 1;
    }
    n
}

/// Stack probe required by the MSVC ABI for frames larger than a page.
/// rax holds the frame size. Every page below rsp is touched and all registers are kept.
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn __chkstk() {
    core::arch::naked_asm!(
        "push rcx",
        "push rax",
        "cmp rax, 0x1000",
        "lea rcx, [rsp + 24]",
        "jb 2f",
        "3:",
        "sub rcx, 0x1000",
        "test [rcx], rcx",
        "sub rax, 0x1000",
        "cmp rax, 0x1000",
        "ja 3b",
        "2:",
        "sub rcx, rax",
        "test [rcx], rcx",
        "pop rax",
        "pop rcx",
        "ret",
    );
}
