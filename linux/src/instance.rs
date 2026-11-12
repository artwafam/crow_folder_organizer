//! One running Crow per user. The first instance listens on an abstract Unix
//! socket; later ones (the desktop menu entries, the settings shortcut, the
//! update checker) send it a short request and exit.

use std::io::{Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};

fn name(what: &str) -> SocketAddr {
    // One instance per user and data folder (CROW_DATA picks another one,
    // which also keeps test runs away from the real instance).
    let uid = unsafe { libc::getuid() };
    let home = crate::util::fnv(0, &crate::store::root().iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>());
    SocketAddr::from_abstract_name(format!("crowfo-{uid}-{home:x}-{what}").as_bytes()).expect("socket name")
}

/// Becomes the running instance, or fails when one already runs.
pub fn listen() -> Result<UnixListener, ()> {
    let l = UnixListener::bind_addr(&name("app")).map_err(|_| ())?;
    l.set_nonblocking(true).map_err(|_| ())?;
    Ok(l)
}

/// Sends a request to the running instance.
pub fn send(msg: &str) -> bool {
    match UnixStream::connect_addr(&name("app")) {
        Ok(mut s) => s.write_all(msg.as_bytes()).is_ok(),
        Err(_) => false,
    }
}

pub fn is_running() -> bool {
    UnixStream::connect_addr(&name("app")).is_ok()
}

/// Requests waiting on the socket.
pub fn accept(l: &UnixListener) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok((s, _)) = l.accept() {
        let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(200)));
        let mut buf = String::new();
        if s.take(256).read_to_string(&mut buf).is_ok() {
            out.push(buf.trim().to_string());
        }
    }
    out
}

/// A named lock held for as long as the returned value lives (like a named
/// mutex on Windows), or None when another process holds it.
pub fn lock(what: &str) -> Option<UnixListener> {
    UnixListener::bind_addr(&name(what)).ok()
}
