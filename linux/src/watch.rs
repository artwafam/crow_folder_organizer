//! Watches the data folder and each folder inside it with inotify, so changes
//! from sync tools or another editor show up right away.

pub struct Watcher {
    fd: i32,
    root: String,
}

const MASK: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_MOVED_FROM | libc::IN_CREATE | libc::IN_DELETE | libc::IN_DELETE_SELF;

impl Watcher {
    pub fn new(root: &str) -> Watcher {
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        let mut w = Watcher { fd, root: root.to_string() };
        w.rewatch();
        w
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }

    fn add(&self, path: &str) {
        if let Ok(c) = std::ffi::CString::new(path) {
            // adding a path twice just updates its watch
            unsafe { libc::inotify_add_watch(self.fd, c.as_ptr(), MASK) };
        }
    }

    /// Watches the root and every folder in it, including new ones.
    pub fn rewatch(&mut self) {
        if self.fd < 0 {
            return;
        }
        self.add(&self.root);
        if let Ok(rd) = std::fs::read_dir(&self.root) {
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    self.add(&e.path().to_string_lossy());
                }
            }
        }
    }

    /// Reads every pending event. True when something changed.
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        let mut buf = [0u8; 4096];
        loop {
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            any = true;
        }
        // let a burst of writes (sync, an editor saving) settle first
        if any {
            std::thread::sleep(std::time::Duration::from_millis(30));
            loop {
                let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n <= 0 {
                    break;
                }
            }
        }
        any
    }
}
