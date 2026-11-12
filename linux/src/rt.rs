//! The bits of ../src/rt.rs the shared code calls. The rest (allocator, panic
//! handler, memory intrinsics) comes from std on Linux.

pub unsafe fn wcslen(s: *const u16) -> usize {
    let mut n = 0;
    while *s.add(n) != 0 {
        n += 1;
    }
    n
}
