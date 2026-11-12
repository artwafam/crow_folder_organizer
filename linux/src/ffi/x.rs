//! The X11 connection shared by everything: atoms, the 32-bit visual used for
//! transparent windows, cursors and a queue for events read out of order.

use std::collections::VecDeque;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_STATE,
        WM_CHANGE_STATE,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_WINDOW_TYPE_DND,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_BELOW,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_DESKTOP,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_ICON,
        _NET_WM_USER_TIME,
        _NET_WM_MOVERESIZE,
        _NET_ACTIVE_WINDOW,
        _NET_WORKAREA,
        _NET_CURRENT_DESKTOP,
        _NET_SHOWING_DESKTOP,
        _NET_SUPPORTING_WM_CHECK,
        _NET_FRAME_EXTENTS,
        _MOTIF_WM_HINTS,
        _KDE_NET_WM_BLUR_BEHIND_REGION,
        UTF8_STRING,
        STRING,
        TEXT,
        TARGETS,
        CLIPBOARD,
        CROW_SELECTION,
        XdndAware,
        XdndEnter,
        XdndPosition,
        XdndStatus,
        XdndLeave,
        XdndDrop,
        XdndFinished,
        XdndSelection,
        XdndTypeList,
        XdndActionCopy,
        XdndActionMove,
        XdndActionLink,
        XdndActionPrivate,
        XdndActionAsk,
        TEXT_URI_LIST: b"text/uri-list",
    }
}

pub struct X {
    pub conn: RustConnection,
    pub screen: usize,
    pub root: Window,
    pub atoms: Atoms,
    /// 32-bit TrueColor visual and its colormap, for per-pixel alpha.
    pub argb: Visualid,
    pub colormap: Colormap,
    pub root_depth: u8,
    pub root_visual: Visualid,
    pub dpi: u32,
    pub has_shape: bool,
    pub has_randr: bool,
    pub cursors: x11rb::cursor::Handle,
    /// Events read while waiting for a specific reply (clipboard, XDND).
    pub pending: VecDeque<Event>,
    pub max_request: usize,
    /// The connection to the display was lost.
    pub broken: bool,
}

static mut XS: Option<X> = None;

pub fn x() -> &'static mut X {
    unsafe {
        let s = &mut *core::ptr::addr_of_mut!(XS);
        if s.is_none() {
            *s = Some(open().unwrap_or_else(|| {
                eprintln!("Crow FO needs an X11 display (DISPLAY is not set or can't be opened).");
                std::process::exit(1)
            }));
        }
        s.as_mut().unwrap()
    }
}

pub fn is_open() -> bool {
    unsafe { (*core::ptr::addr_of!(XS)).is_some() }
}

fn open() -> Option<X> {
    let (conn, screen) = RustConnection::connect(None).ok()?;
    let atoms = Atoms::new(&conn).ok()?.reply().ok()?;
    let s = &conn.setup().roots[screen];
    let root = s.root;
    let (root_depth, root_visual) = (s.root_depth, s.root_visual);
    let mut argb = 0;
    for d in &s.allowed_depths {
        if d.depth == 32 {
            if let Some(v) = d.visuals.iter().find(|v| v.class == VisualClass::TRUE_COLOR) {
                argb = v.visual_id;
            }
        }
    }
    if argb == 0 {
        eprintln!("Crow FO needs a 32-bit visual (a compositing desktop).");
        return None;
    }
    let colormap = conn.generate_id().ok()?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, root, argb).ok()?;
    let db = x11rb::resource_manager::new_from_default(&conn).ok();
    let dpi = db
        .as_ref()
        .and_then(|d| d.get_value::<String>("Xft.dpi", "").ok().flatten())
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|v| v.round() as u32)
        .filter(|&v| (48..=480).contains(&v))
        .unwrap_or(96);
    let empty = x11rb::resource_manager::Database::new_from_default(&GetPropertyReply::default(), std::ffi::OsString::new());
    let cursors = x11rb::cursor::Handle::new(&conn, screen, db.as_ref().unwrap_or(&empty)).ok()?.reply().ok()?;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::shape::ConnectionExt as _;
    let has_shape = conn.shape_query_version().ok().and_then(|c| c.reply().ok()).is_some();
    let has_randr = conn.randr_query_version(1, 5).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.major_version > 1 || r.minor_version >= 5);
    let max_request = conn.maximum_request_bytes();
    // Window manager and desktop changes arrive as root properties.
    conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE)).ok()?;
    Some(X {
        conn,
        screen,
        root,
        atoms,
        argb,
        colormap,
        root_depth,
        root_visual,
        dpi,
        has_shape,
        has_randr,
        cursors,
        pending: VecDeque::new(),
        max_request,
        broken: false,
    })
}

impl X {
    pub fn flush(&self) {
        let _ = self.conn.flush();
    }

    /// The next event, from the out-of-order queue first.
    pub fn poll(&mut self) -> Option<Event> {
        if let Some(e) = self.pending.pop_front() {
            return Some(e);
        }
        match self.conn.poll_for_event() {
            Ok(e) => e,
            Err(_) => {
                // The display is gone (logout or a crashed X server).
                self.broken = true;
                None
            }
        }
    }

    /// Waits up to `ms` for an event that `want` accepts. Other events are
    /// kept for later, in order.
    pub fn wait_for(&mut self, ms: u64, want: &dyn Fn(&Event) -> bool) -> Option<Event> {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        self.flush();
        loop {
            while let Ok(Some(e)) = self.conn.poll_for_event() {
                if want(&e) {
                    return Some(e);
                }
                self.pending.push_back(e);
            }
            let left = end.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            let mut fd = libc::pollfd { fd: self.fd(), events: libc::POLLIN, revents: 0 };
            unsafe { libc::poll(&mut fd, 1, left.as_millis().clamp(1, 1000) as i32) };
        }
    }

    /// Whether events are waiting inside the connection (read while waiting
    /// for a reply), where polling the socket wouldn't see them.
    pub fn buffered(&mut self) -> bool {
        if let Ok(Some(e)) = self.conn.poll_for_event() {
            self.pending.push_back(e);
        }
        !self.pending.is_empty()
    }

    pub fn fd(&self) -> i32 {
        use std::os::fd::AsRawFd;
        self.conn.stream().as_raw_fd()
    }

    pub fn atom(&self, name: &str) -> Atom {
        self.conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.atom)
    }

    pub fn prop32(&self, w: Window, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
        self.conn
            .get_property(false, w, prop, ty, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    pub fn prop_bytes(&self, w: Window, prop: Atom, ty: impl Into<Atom>) -> Vec<u8> {
        self.conn
            .get_property(false, w, prop, ty, 0, 1 << 22)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default()
    }

    /// An EWMH client message to the root window (window manager requests).
    pub fn wm_message(&self, w: Window, ty: Atom, data: [u32; 5]) {
        let ev = ClientMessageEvent::new(32, w, ty, data);
        let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev);
    }

    pub fn cursor(&self, name: &str) -> Cursor {
        self.cursors.load_cursor(&self.conn, name).unwrap_or(0)
    }
}
