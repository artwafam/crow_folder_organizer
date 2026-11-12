//! HTTPS downloads and SHA-256. Only the update check uses them, and it runs
//! in its own short lived process, so the app never touches the network.
//! Downloads go through curl (or wget), which every Linux desktop has and
//! which follows the system's certificates and proxy settings.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

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

fn agent() -> String {
    format!("CrowFO/{} (Linux)", env!("CARGO_PKG_VERSION"))
}

/// The size of what `url` serves after redirects, when the server says it.
fn size_of(url: &str, accept: &str) -> Option<usize> {
    let o = Command::new("curl")
        .args(["-sIL", "--proto", "=https", "--proto-redir", "=https", "--max-time", "20", "-A", &agent(), "-H", &format!("Accept: {accept}"), url])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&o.stdout);
    // the last response is the one after every redirect
    t.lines().filter_map(|l| l.split_once(':')).filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).last().and_then(|(_, v)| v.trim().parse().ok())
}

/// Downloads an https URL, following redirects. Gives up past `max` bytes.
pub fn get(url: &str, accept: &str, max: usize) -> Result<Vec<u8>, Error> {
    if !url.starts_with("https://") {
        return Err(Error::Refused);
    }
    let curl = crate::ffi::has_program("curl");
    let mut cmd = if curl {
        let mut c = Command::new("curl");
        c.args([
            "-sSL",
            "--fail",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "120",
            "--max-filesize",
            &max.to_string(),
            "-A",
            &agent(),
            "-H",
            &format!("Accept: {accept}"),
            url,
        ]);
        c
    } else if crate::ffi::has_program("wget") {
        let mut c = Command::new("wget");
        c.args(["-q", "-O", "-", "--https-only", "--timeout=30", "-U", &agent(), &format!("--header=Accept: {accept}"), url]);
        c
    } else {
        return Err(Error::Network);
    };
    let total = if curl && max > 1 << 20 { size_of(url, accept) } else { None };
    if total.is_some_and(|t| t > max) {
        return Err(Error::Refused);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| Error::Network)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    let mut r = child.stdout.take().ok_or(Error::Network)?;
    loop {
        let n = r.read(&mut buf).map_err(|_| Error::Network)?;
        if n == 0 {
            break;
        }
        if out.len() + n > max {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Refused);
        }
        out.extend_from_slice(&buf[..n]);
        if let Some(t) = total.filter(|&t| t > 0) {
            PROGRESS.store(((out.len() as u64 * 1000) / t as u64).min(1000) as u32, Ordering::Relaxed);
        }
    }
    let status = child.wait().map_err(|_| Error::Network)?;
    match status.code() {
        Some(0) => Ok(out),
        // curl: 22 is an HTTP error and 63 a file over --max-filesize; wget: 8 is a server error
        Some(22) | Some(63) if curl => Err(Error::Refused),
        Some(8) if !curl => Err(Error::Refused),
        _ => Err(Error::Network),
    }
}

pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    use sha2::Digest;
    Some(sha2::Sha256::digest(data).into())
}
