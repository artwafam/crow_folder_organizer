//! Files dragged from file managers, the desktop or the app menu onto folders,
//! through the XDND protocol. It feeds the same ole_enter / ole_over /
//! ole_leave / ole_drop calls as the Windows drop target, so the folders behave
//! the same (springing open, insert marks, moving or linking).

use crate::ffi::x::x;
use crate::ffi::*;
use crate::util::*;
use alloc::vec::Vec;
use core::ffi::c_void;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;

struct Dnd {
    source: u32,
    target: u32,
    version: u32,
    types: Vec<Atom>,
    fetched: bool,
    accepted: bool,
    action: Atom,
}

static mut D: Option<Dnd> = None;

/// Passed to RegisterDragDrop; the window layer marks the window XdndAware.
pub fn target_ptr() -> *mut c_void {
    core::ptr::null_mut()
}

pub fn init() {
    crate::ffi::win::set_client_hook(on_event);
}

fn on_event(e: &Event) -> bool {
    let Event::ClientMessage(ev) = e else { return false };
    let a = &x().atoms;
    let d = ev.data.as_data32();
    unsafe {
        if ev.type_ == a.XdndEnter {
            enter(ev.window, &d);
        } else if ev.type_ == a.XdndPosition {
            position(ev.window, &d);
        } else if ev.type_ == a.XdndLeave {
            leave(d[0]);
        } else if ev.type_ == a.XdndDrop {
            dropped(d[0], d[2]);
        } else {
            return false;
        }
    }
    true
}

unsafe fn enter(target: u32, d: &[u32; 5]) {
    let xs = x();
    let source = d[0];
    let version = d[1] >> 24;
    let types = if d[1] & 1 != 0 {
        xs.prop32(source, xs.atoms.XdndTypeList, AtomEnum::ATOM)
    } else {
        d[2..5].iter().copied().filter(|&t| t != 0).collect()
    };
    *core::ptr::addr_of_mut!(D) = Some(Dnd { source, target, version, types, fetched: false, accepted: false, action: 0 });
}

fn state() -> Option<&'static mut Dnd> {
    unsafe { (*core::ptr::addr_of_mut!(D)).as_mut() }
}

/// The dragged files, read from the source as a text/uri-list.
fn fetch(time: u32) -> Vec<WString> {
    let xs = x();
    let Some(s) = state() else { return Vec::new() };
    if !s.types.contains(&xs.atoms.TEXT_URI_LIST) {
        return Vec::new();
    }
    let me = crate::ffi::win::utility_window();
    let prop = xs.atoms.CROW_SELECTION;
    let _ = xs.conn.convert_selection(me, xs.atoms.XdndSelection, xs.atoms.TEXT_URI_LIST, prop, time);
    let sel = xs.atoms.XdndSelection;
    let Some(Event::SelectionNotify(n)) = xs.wait_for(1500, &|e| matches!(e, Event::SelectionNotify(n) if n.requestor == me && n.selection == sel))
    else {
        return Vec::new();
    };
    if n.property == x11rb::NONE {
        return Vec::new();
    }
    let data = xs.prop_bytes(me, prop, AtomEnum::ANY);
    let _ = xs.conn.delete_property(me, prop);
    parse_uris(&String::from_utf8_lossy(&data))
}

pub fn parse_uris(t: &str) -> Vec<WString> {
    let mut out = Vec::new();
    for l in t.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let Some(rest) = l.strip_prefix("file://") else { continue };
        // file://host/path: only this machine's files
        let path = match rest.find('/') {
            Some(0) => rest,
            Some(i) if &rest[..i] == "localhost" || rest[..i] == crate::store::computer_name() => &rest[i..],
            _ => continue,
        };
        out.push(to_w(&percent_decode(path)));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn action_for(effect: u32, wanted: Atom) -> Atom {
    let a = &x().atoms;
    // Keep what the source asked for when it is a plain one: Crow moves or
    // links by itself, and some sources only allow copying.
    if wanted == a.XdndActionCopy || wanted == a.XdndActionMove || wanted == a.XdndActionLink {
        return wanted;
    }
    match effect {
        DROPEFFECT_MOVE => a.XdndActionMove,
        DROPEFFECT_LINK => a.XdndActionLink,
        _ => a.XdndActionCopy,
    }
}

unsafe fn position(target: u32, d: &[u32; 5]) {
    let Some(s) = state() else { return };
    s.target = target;
    let pt = POINT { x: (d[2] >> 16) as i16 as i32, y: (d[2] & 0xFFFF) as i16 as i32 };
    let time = d[3];
    let a = crate::app::app();
    let allowed = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
    let eff = if !s.fetched {
        s.fetched = true;
        let files = fetch(time);
        a.ole_enter(files, pt, allowed)
    } else {
        a.ole_over(pt, allowed)
    };
    let s = state().unwrap();
    s.accepted = eff != DROPEFFECT_NONE;
    s.action = if s.accepted { action_for(eff, d[4]) } else { 0 };
    send(s.source, x().atoms.XdndStatus, [s.target, u32::from(s.accepted) | 2, 0, 0, s.action]);
}

unsafe fn leave(source: u32) {
    if state().is_some_and(|s| s.source == source) {
        crate::app::app().ole_leave();
        *core::ptr::addr_of_mut!(D) = None;
    }
}

unsafe fn dropped(source: u32, _time: u32) {
    let Some(s) = state() else { return };
    if s.source != source {
        return;
    }
    let (target, accepted, action, version) = (s.target, s.accepted, s.action, s.version);
    if accepted {
        let mut p = POINT::default();
        GetCursorPos(&mut p);
        crate::app::app().ole_drop(p);
    } else {
        crate::app::app().ole_leave();
    }
    let data = if version >= 5 { [target, u32::from(accepted), if accepted { action } else { 0 }, 0, 0] } else { [target, 0, 0, 0, 0] };
    send(source, x().atoms.XdndFinished, data);
    *core::ptr::addr_of_mut!(D) = None;
}

fn send(to: u32, ty: Atom, data: [u32; 5]) {
    let xs = x();
    let ev = ClientMessageEvent::new(32, to, ty, data);
    let _ = xs.conn.send_event(false, to, EventMask::NO_EVENT, ev);
    xs.flush();
}
