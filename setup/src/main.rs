//! CrowSetup.exe: installs the crow.exe it carries for the current user.
#![no_std]
#![no_main]

use crow::ffi::*;

#[no_mangle]
pub extern "system" fn entry() -> ! {
    unsafe {
        crow::installer::harden();
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        OleInitialize(core::ptr::null_mut());
        // the app travels inside this file as resource RCDATA #1
        let inst = GetModuleHandleW(core::ptr::null());
        let r = FindResourceW(inst, 1, 10);
        let payload: &'static [u8] = if r != 0 {
            let data = LockResource(LoadResource(inst, r));
            core::slice::from_raw_parts(data, SizeofResource(inst, r) as usize)
        } else {
            &[]
        };
        crow::installer::setup(payload);
        ExitProcess(0)
    }
}
