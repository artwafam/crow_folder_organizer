//! HTTPS downloads through WinHTTP and SHA-256 through Windows CNG. Only the
//! update check uses them, and it runs in its own short lived process.

use crate::ffi::*;
use crate::util::*;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};

/// How far the running download is, in thousandths. Stays 0 when the server
/// doesn't send the size.
pub static PROGRESS: AtomicU32 = AtomicU32::new(0);

/// Why a request failed.
pub enum Error {
    /// No connection, a timeout or anything else before an answer came back.
    Network,
    /// The server answered with something other than 200 or sent too much.
    Refused,
}

/// Downloads an https URL, following redirects. Gives up past `max` bytes.
pub fn get(url: &str, accept: &str, max: usize) -> Result<Vec<u8>, Error> {
    let rest = url.strip_prefix("https://").ok_or(Error::Refused)?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let mut agent = String::from("CrowFO/");
    agent.push_str(env!("CARGO_PKG_VERSION"));
    unsafe {
        // 4 is WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, which follows the system proxy
        let s = WinHttpOpen(wz(&to_w(&agent)).as_ptr(), 4, null(), null(), 0);
        if s == 0 {
            return Err(Error::Network);
        }
        WinHttpSetTimeouts(s, 10_000, 10_000, 15_000, 30_000);
        let c = WinHttpConnect(s, wz(&to_w(host)).as_ptr(), 443, 0);
        let mut out = Err(Error::Network);
        if c != 0 {
            // 0x800000 is WINHTTP_FLAG_SECURE
            let r = WinHttpOpenRequest(c, w!("GET").as_ptr(), wz(&to_w(path)).as_ptr(), null(), null(), null(), 0x0080_0000);
            if r != 0 {
                out = read(r, accept, max);
                WinHttpCloseHandle(r);
            }
            WinHttpCloseHandle(c);
        }
        WinHttpCloseHandle(s);
        out
    }
}

unsafe fn read(r: isize, accept: &str, max: usize) -> Result<Vec<u8>, Error> {
    const NUMBER: u32 = 0x2000_0000; // WINHTTP_QUERY_FLAG_NUMBER
    let mut head = String::from("Accept: ");
    head.push_str(accept);
    let head = to_w(&head);
    if WinHttpSendRequest(r, head.as_ptr(), head.len() as u32, null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(r, null_mut()) == 0 {
        return Err(Error::Network);
    }
    let mut code = 0u32;
    let mut len = 4u32;
    WinHttpQueryHeaders(r, 19 | NUMBER, null(), &mut code as *mut u32 as _, &mut len, null_mut());
    if code != 200 {
        return Err(Error::Refused);
    }
    let mut total = 0u32;
    len = 4;
    let sized = WinHttpQueryHeaders(r, 5 | NUMBER, null(), &mut total as *mut u32 as _, &mut len, null_mut()) != 0 && total > 0;
    if sized && total as usize > max {
        return Err(Error::Refused);
    }
    PROGRESS.store(0, Ordering::Relaxed);
    let mut data: Vec<u8> = Vec::new();
    loop {
        let mut n = 0u32;
        if WinHttpQueryDataAvailable(r, &mut n) == 0 {
            return Err(Error::Network);
        }
        if n == 0 {
            break;
        }
        let at = data.len();
        if at + n as usize > max {
            return Err(Error::Refused);
        }
        data.resize(at + n as usize, 0);
        let mut got = 0u32;
        if WinHttpReadData(r, data[at..].as_mut_ptr() as _, n, &mut got) == 0 {
            return Err(Error::Network);
        }
        data.truncate(at + got as usize);
        if sized {
            PROGRESS.store((data.len() as u64 * 1000 / total as u64).min(1000) as u32, Ordering::Relaxed);
        }
    }
    Ok(data)
}

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    unsafe {
        let mut alg = 0isize;
        if BCryptOpenAlgorithmProvider(&mut alg, w!("SHA256").as_ptr(), null(), 0) != 0 {
            return None;
        }
        let mut out = [0u8; 32];
        let mut h = 0isize;
        let mut ok = BCryptCreateHash(alg, &mut h, null_mut(), 0, null(), 0, 0) == 0;
        if ok {
            ok = BCryptHashData(h, data.as_ptr(), data.len() as u32, 0) == 0 && BCryptFinishHash(h, out.as_mut_ptr(), 32, 0) == 0;
            BCryptDestroyHash(h);
        }
        BCryptCloseAlgorithmProvider(alg, 0);
        if ok { Some(out) } else { None }
    }
}
