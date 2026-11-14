//! Quickshell instance registration and IPC socket, so `qs -c <config> ipc call <target> <fn>`
//! (and Hyprland's `qs ... ipc call TEST_ALIVE || fuzzel` fallbacks) reach the native shell.
//! Layout and wire format follow quickshell's `QsPaths` and `IpcCommand` (QDataStream, Qt 6).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::symlink;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

/// An IPC command, answered on the main thread.
pub struct Call {
    pub target: String,
    /// Function, property or signal name (empty for [`Op::Kill`]).
    pub function: String,
    pub args: Vec<String>,
    pub op: Op,
}

pub enum Op {
    /// `StringCallCommand` (`qs ipc call`).
    Call,
    /// `QueryMetadataCommand` (`qs ipc show`): answer with [`CallResult::Targets`].
    Query,
    /// `StringPropReadCommand` (`qs ipc prop get`): answer with [`CallResult::Value`].
    Prop,
    /// `SignalListenCommand` (`qs ipc listen|wait`): answer with [`CallResult::Listening`], then
    /// send each emission through the [`Listener`].
    Listen(Listener),
    /// `qs kill`: quit through the main loop.
    Kill,
}

/// `WireTargetDefinition`: what `qs ipc show` lists for one target.
pub struct TargetDef {
    pub name: String,
    pub functions: Vec<FnDef>,
    /// (name, type)
    pub properties: Vec<(String, String)>,
    /// (name, argument name, argument type); both empty for no argument.
    pub signals: Vec<(String, String, String)>,
}

#[derive(Clone)]
pub struct FnDef {
    pub name: String,
    pub ret: String,
    /// (name, type)
    pub args: Vec<(String, String)>,
}

fn put_pair(out: &mut Vec<u8>, (a, b): &(String, String)) {
    put_str(out, a);
    put_str(out, b);
}

fn put_fn(out: &mut Vec<u8>, f: &FnDef) {
    put_str(out, &f.name);
    put_str(out, &f.ret);
    out.extend((f.args.len() as u32).to_be_bytes());
    f.args.iter().for_each(|a| put_pair(out, a));
}

fn put_target(out: &mut Vec<u8>, t: &TargetDef) {
    put_str(out, &t.name);
    out.extend((t.functions.len() as u32).to_be_bytes());
    t.functions.iter().for_each(|f| put_fn(out, f));
    out.extend((t.properties.len() as u32).to_be_bytes());
    t.properties.iter().for_each(|p| put_pair(out, p));
    out.extend((t.signals.len() as u32).to_be_bytes());
    for (n, an, at) in &t.signals {
        put_str(out, n);
        put_str(out, an);
        put_str(out, at);
    }
}

pub enum CallResult {
    /// Return value, `None` for a void function.
    Done(Option<String>),
    TargetNotFound,
    EntryNotFound,
    /// Arguments that don't fit the declared parameters: the function, whether the count is
    /// wrong, else the index of the argument that failed to parse.
    ArgParse(FnDef, bool, u8),
    /// The handler threw: the error message.
    Error(String),
    /// A property's value, as `IpcValueSlot::toString` for its declared type.
    Value(String),
    /// The signal exists; emissions follow on the [`Op::Listen`] channel.
    Listening,
    /// Enabled targets matching the query (all of them for an empty target).
    Targets(Vec<TargetDef>),
}

/// This instance's IPC server, served on the main thread: its listening socket and every
/// connection are non-blocking and watched by one epoll fd ([`Server::fd`]) that the main loop
/// polls, so requests are read, answered and written without threads or channels. Until the
/// loop runs, clients wait in the socket's backlog.
pub struct Server {
    conns: Rc<RefCell<Conns>>,
    _registered: Registration,
}

/// What [`start`] made of this instance under the run dir, removed again when it is dropped:
/// a start that fails after registering (a refused tree, no compositor) leaves no dead
/// instance for `qs list` and `qs ipc` to show. The log files stay, as Quickshell's do.
#[derive(Default)]
struct Registration {
    lock: Option<std::fs::File>,
    paths: Vec<std::path::PathBuf>,
    /// The instance directory, removed with its logs by [`Server::unregister`].
    dir: Option<std::path::PathBuf>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // Links first, so no client finds the instance while its socket and lock go away.
        for p in self.paths.iter().rev() {
            let _ = std::fs::remove_file(p);
        }
        drop(self.lock.take());
    }
}

impl Server {
    fn new(listener: UnixListener, registered: Registration) -> std::io::Result<Self> {
        listener.set_nonblocking(true)?;
        // SAFETY: a new epoll instance, owned below.
        let ep = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if ep < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `ep` is the fresh descriptor epoll_create1 returned; nothing else owns it.
        let ep = unsafe { OwnedFd::from_raw_fd(ep) };
        ep_ctl(
            &ep,
            libc::EPOLL_CTL_ADD,
            listener.as_raw_fd(),
            libc::EPOLLIN as u32,
            LISTENER,
        )?;
        Ok(Self {
            conns: Rc::new(RefCell::new(Conns {
                ep,
                listener,
                map: HashMap::new(),
                next: LISTENER + 1,
                reserve: reserve_fd().ok(),
            })),
            _registered: registered,
        })
    }

    /// A server on a fresh socket at `path`, not registered as an instance (tests).
    #[doc(hidden)]
    pub fn bind_unregistered(path: &Path) -> std::io::Result<Self> {
        Self::new(UnixListener::bind(path)?, Registration::default())
    }

    /// The epoll fd to watch for readability; then call [`Server::requests`].
    pub fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        // SAFETY: the epoll fd lives as long as `self`.
        unsafe { std::os::fd::BorrowedFd::borrow_raw(self.conns.borrow().ep.as_raw_fd()) }
    }

    /// Accepts new connections, reads what clients sent, writes out pending answers and
    /// returns the complete requests, each to be answered with [`Server::reply`]. A client's
    /// next pipelined request is returned by the call after its previous one was answered, so
    /// call this until it returns nothing.
    pub fn requests(&self) -> Vec<Request> {
        self.conns.borrow_mut().poll(&self.conns)
    }

    /// Open client connections: requests not answered yet and listeners.
    pub fn connections(&self) -> usize {
        self.conns.borrow().map.len()
    }

    /// Answers `req` with the main loop's `result`.
    pub fn reply(&self, req: Request, result: CallResult) {
        let mut c = self.conns.borrow_mut();
        match req.reply {
            Reply::Close => c.close(req.conn),
            Reply::Encode(name) => {
                let listening = matches!(result, CallResult::Listening);
                let out = encode(&req.call, &name, result);
                c.send(req.conn, &out);
                if let Some(conn) = c.map.get_mut(&req.conn) {
                    conn.busy = false;
                    conn.listening = listening;
                }
            }
        }
    }

    /// Unregisters the instance at an orderly exit and removes its run directory with its logs
    /// (the XDG runtime dir is a tmpfs, for short-lived files only): only a shell that died
    /// leaves its directory, for `qs log`. With `keep_logs` (an exit with an error status) the
    /// directory and logs stay and only the links, socket and lock go.
    pub fn unregister(mut self, keep_logs: bool) {
        let dir = self._registered.dir.take();
        drop(self);
        if let Some(dir) = dir.filter(|_| !keep_logs) {
            crate::rilog::close();
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// One request read from a client.
pub struct Request {
    conn: u64,
    pub call: Call,
    reply: Reply,
}

enum Reply {
    /// `qs kill`: no answer, the connection closes.
    Close,
    /// Encode the answer; for `ipc show` the member name asked about.
    Encode(String),
}

/// A remote `qs ipc listen|wait` stream: [`Listener::send`] writes one emission
/// (`SignalResponse`) to its client.
#[derive(Clone)]
pub struct Listener {
    conns: Weak<RefCell<Conns>>,
    conn: u64,
}

impl Listener {
    /// Whether the client is still connected.
    pub fn alive(&self) -> bool {
        self.conns
            .upgrade()
            .is_some_and(|c| c.borrow().map.contains_key(&self.conn))
    }

    /// Sends one emission; false once the client is gone.
    pub fn send(&self, value: &str) -> bool {
        let Some(c) = self.conns.upgrade() else {
            return false;
        };
        let mut out = vec![4];
        put_str(&mut out, value);
        c.borrow_mut().send(self.conn, &out)
    }

    /// Ends the stream (its `IpcHandler` was destroyed): the client sees EOF.
    pub fn close(&self) {
        if let Some(c) = self.conns.upgrade() {
            c.borrow_mut().close(self.conn);
        }
    }
}

/// Register this process as a live instance of `config` (shell.qml) and serve its IPC socket.
/// Called first thing at startup, before the Wayland connection, the GPU and the QML, as
/// Quickshell does: `qs ipc call` finds the instance from then on, and its calls wait until
/// the main loop answers them. An error names the step and path that failed; whatever was
/// registered before it is removed again.
pub fn start(config: &Path) -> std::io::Result<Server> {
    use std::io::{Error, ErrorKind};
    let ctx = |what: &str, p: &Path| {
        let what = format!("{what} {}", p.display());
        move |e: Error| Error::new(e.kind(), format!("{what}: {e}"))
    };
    let base = base_run_dir();
    let config = clean_absolute(config).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("{}: no absolute path", config.display()),
        )
    })?;
    let path_id = md5_hex(config.as_bytes());
    let pid = std::process::id();
    let (id, now) = instance();
    let inst = base.join("by-id").join(id);
    let shell_dir = base.join("by-shell").join(&path_id);
    for d in [
        &inst,
        &shell_dir,
        &base.join("by-pid"),
        &base.join("by-path"),
    ] {
        std::fs::create_dir_all(d).map_err(ctx("create", d))?;
    }

    let display = display_connection();
    let mut info = Vec::new();
    for s in [id, &config, &path_id, "org.rustshell"] {
        put_str(&mut info, s);
    }
    let ms = now.as_millis() as i64;
    let wall = ms + i64::from(qml::tz::local_ahead(ms.div_euclid(1000))) * 1000;
    info.extend((2_440_588 + wall.div_euclid(86_400_000)).to_be_bytes());
    info.extend((wall.rem_euclid(86_400_000) as u32).to_be_bytes());
    info.push(0); // Qt::LocalTime, as Quickshell's `QDateTime::currentDateTime()`
    info.extend((pid as i32).to_be_bytes());
    put_str(&mut info, &display);

    let mut registered = Registration::default();
    registered.dir = Some(inst.clone());
    let lock_path = inst.join("instance.lock");
    let mut lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(ctx("open", &lock_path))?;
    // A POSIX record lock, not an OFD lock: Quickshell's `qs` client reads the holder's pid
    // with F_GETLK, which reports -1 for an OFD lock (fcntl(2)). The file is truncated only
    // once the lock is ours, so a live holder's info is never clobbered.
    if !set_lock(&lock, libc::F_WRLCK) {
        return Err(ctx("lock", &lock_path)(Error::last_os_error()));
    }
    registered.paths.push(lock_path.clone());
    lock.set_len(0)
        .and_then(|()| lock.write_all(&info))
        .map_err(ctx("write", &lock_path))?;
    // Held as long as the instance is registered: `qs` treats an unlocked instance as dead.
    registered.lock = Some(lock);

    // The socket listens before any link makes the instance findable: a client that finds it
    // connects at once, and its call waits in the backlog until the main loop answers.
    let sock = inst.join("ipc.sock");
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).map_err(ctx("bind", &sock))?;
    registered.paths.push(sock);

    let inst_c = std::fs::canonicalize(&inst).map_err(ctx("resolve", &inst))?;
    for link in [
        shell_dir.join(id),
        base.join("by-pid").join(pid.to_string()),
    ] {
        relink(&link, &inst_c).map_err(ctx("link", &link))?;
        registered.paths.push(link);
    }
    // Shared by every instance of this config, as Quickshell's: it stays.
    let by_path = base.join("by-path").join(&path_id);
    let sd = std::fs::canonicalize(&shell_dir).map_err(ctx("resolve", &shell_dir))?;
    relink(&by_path, &sd).map_err(ctx("link", &by_path))?;
    prune_dead(&base, &shell_dir, id);
    Server::new(listener, registered).map_err(|e| Error::new(e.kind(), format!("epoll: {e}")))
}

/// `fcntl(F_SETLK)` of the whole file; `F_UNLCK` releases it.
fn set_lock(f: &std::fs::File, ty: libc::c_int) -> bool {
    let fl = libc::flock {
        l_type: ty as _,
        l_whence: libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // SAFETY: F_SETLK on an open file takes a pointer to an initialized flock (fcntl(2)).
    unsafe { libc::fcntl(f.as_raw_fd(), libc::F_SETLK, &fl) == 0 }
}

/// Points `link` at `to` atomically: a fresh symlink under a temporary name renamed over the
/// old one (rename(2) replaces atomically), so a client never finds the link missing.
fn relink(link: &Path, to: &Path) -> std::io::Result<()> {
    let mut tmp = link.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    let _ = std::fs::remove_file(&tmp);
    symlink(to, &tmp)?;
    std::fs::rename(&tmp, link).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Whether the instance directory `dir` is registered but no process holds its lock.
fn is_dead(dir: &Path) -> bool {
    let Ok(f) = std::fs::File::open(dir.join("instance.lock")) else {
        return false;
    };
    let mut fl = libc::flock {
        l_type: libc::F_WRLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // SAFETY: F_GETLK on an open file takes a pointer to an initialized flock it overwrites.
    let ok = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETLK, &mut fl) } == 0;
    ok && fl.l_type == libc::F_UNLCK as libc::c_short
}

/// Dead instances of this config kept under the run dir (a tmpfs): only the newest one stays,
/// so `qs log` can still show why the last shell died; older ones are removed with their links.
/// A shell that exits in order removes its own directory ([`Server::unregister`]).
const DEAD_KEPT: usize = 1;

fn prune_dead(base: &Path, shell_dir: &Path, own: &str) {
    let Ok(rd) = std::fs::read_dir(shell_dir) else {
        return;
    };
    let mut dead: Vec<(
        std::time::SystemTime,
        std::path::PathBuf,
        std::path::PathBuf,
    )> = rd
        .flatten()
        .filter(|e| e.file_name() != own)
        .filter_map(|e| {
            let link = e.path();
            let dir = std::fs::canonicalize(&link).ok();
            match dir {
                // A link to a removed directory.
                None => {
                    let _ = std::fs::remove_file(&link);
                    None
                }
                Some(dir) if is_dead(&dir) => {
                    let t = std::fs::metadata(dir.join("instance.lock"))
                        .and_then(|m| m.modified())
                        .ok()?;
                    Some((t, link, dir))
                }
                Some(_) => None,
            }
        })
        .collect();
    dead.sort_by_key(|d| std::cmp::Reverse(d.0));
    for (_, link, dir) in dead.into_iter().skip(DEAD_KEPT) {
        remove_instance(base, &dir);
        let _ = std::fs::remove_file(link);
    }
}

/// Removes the instance directory `dir` and the `by-pid` links that point at it.
fn remove_instance(base: &Path, dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(base.join("by-pid")) {
        for e in rd.flatten() {
            if std::fs::canonicalize(e.path()).is_ok_and(|t| t == dir) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// What the image a `Rustshell.reload()` executes needs to go on as this instance
/// ([`resume_instance`]): the instance id and the launch time in milliseconds.
pub fn reload_marker() -> String {
    let (id, launched) = instance();
    format!("{id}/{}", launched.as_millis())
}

/// Makes this process the instance `marker` names ([`reload_marker`]), which the image that
/// re-executed itself into this one registered: a Quickshell reload keeps the process, and
/// with it the instance, its launch time and its log. Call before anything asks for the
/// instance. `false` (and a new instance) unless the id is one of this process, its directory
/// exists and no other process holds its lock.
pub fn resume_instance(marker: &str) -> bool {
    let Some((id, ms)) = marker.split_once('/') else {
        return false;
    };
    let Ok(ms) = ms.parse() else {
        return false;
    };
    let own = base36(u64::from(std::process::id()));
    if id.len() <= own.len()
        || !id.starts_with(&own)
        || !id.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return false;
    }
    if !is_dead(&base_run_dir().join("by-id").join(id)) {
        return false;
    }
    let launched = std::time::Duration::from_millis(ms);
    INSTANCE.set((id.to_owned(), launched)).is_ok()
}

/// Removes the directory and the links of the instance this image goes on as
/// ([`resume_instance`]), for an image that ends before it registers: the image that
/// re-executed itself left them, and nothing else would take them away. What
/// [`Server::unregister`] does at an orderly exit.
pub fn remove_resumed_instance() {
    let Some((id, _)) = INSTANCE.get() else {
        return;
    };
    let base = base_run_dir();
    if let Ok(rd) = std::fs::read_dir(base.join("by-shell")) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path().join(id));
        }
    }
    remove_instance(&base, &base.join("by-id").join(id));
}

/// Most request connections open at once; more are closed at once. Signal listeners count
/// separately ([`MAX_LISTENERS`]), so open `qs ipc listen` clients never lock out calls.
const MAX_CONNS: usize = 64;
const MAX_LISTENERS: usize = 256;
/// Most bytes one request may take: a client that announces a longer string is dropped
/// instead of growing the shell's memory until the string arrives.
const MAX_REQUEST: usize = 1 << 20;
/// Most bytes that may wait for a client that does not read (a stalled `qs ipc listen`) when
/// more output comes: one answer or emission is queued whole however large it is.
const MAX_PENDING_OUT: usize = 1 << 20;
/// A request connection that sent nothing for this long is closed when room is needed.
///
/// A clock by design. A client that connected and never sends its request produces no event
/// to wait for (one that left reads as EOF and is closed at once), so its age is the only
/// sign that it is stuck. Nothing wakes the loop for it: the age is looked at only when a new
/// client arrives and all [`MAX_CONNS`] places are taken ([`Conns::expire_idle`]).
const IDLE: Duration = Duration::from_secs(30);
/// epoll key of the listening socket; connections count up from it.
const LISTENER: u64 = 0;

/// `StringCallResponse` tag for a handler that threw. Quickshell has no such answer (it reports
/// `Completed`), so its client prints "Received invalid IPC response"; ours prints the message.
pub(crate) const CALL_THREW: u8 = 6;

fn ep_ctl(ep: &OwnedFd, op: libc::c_int, fd: i32, events: u32, key: u64) -> std::io::Result<()> {
    let mut ev = libc::epoll_event { events, u64: key };
    // SAFETY: valid epoll and target fds.
    if unsafe { libc::epoll_ctl(ep.as_raw_fd(), op, fd, &mut ev) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

struct Conn {
    s: UnixStream,
    buf: Vec<u8>,
    /// Bytes the next request needs at least before parsing it again is worth it.
    need: usize,
    out: Vec<u8>,
    /// A request is with the main loop: the next one waits until it is answered, in order.
    busy: bool,
    /// The client sent EOF: close once its request is answered.
    eof: bool,
    /// Nothing more to answer: close once `out` is written.
    closing: bool,
    listening: bool,
    last: Instant,
}

struct Conns {
    ep: OwnedFd,
    listener: UnixListener,
    map: HashMap<u64, Conn>,
    next: u64,
    /// A descriptor held back for [`Conns::refuse_pending`], to accept with when the process
    /// is out of them.
    reserve: Option<OwnedFd>,
}

/// The reserve descriptor: `/dev/null`, opened close-on-exec.
fn reserve_fd() -> std::io::Result<OwnedFd> {
    std::fs::File::open("/dev/null").map(OwnedFd::from)
}

impl Conns {
    fn poll(&mut self, me: &Rc<RefCell<Conns>>) -> Vec<Request> {
        self.restore_reserve();
        let mut evs = [libc::epoll_event { events: 0, u64: 0 }; 32];
        loop {
            // SAFETY: `evs` holds `evs.len()` events.
            let n = unsafe {
                libc::epoll_wait(self.ep.as_raw_fd(), evs.as_mut_ptr(), evs.len() as i32, 0)
            };
            if n <= 0 {
                break;
            }
            // Each batch is handled before the next wait: the fds are level-triggered and
            // stay ready until read, written or closed.
            for e in &evs[..n as usize] {
                let (key, events) = (e.u64, e.events);
                if key == LISTENER {
                    self.accept();
                    continue;
                }
                if events & libc::EPOLLOUT as u32 != 0 {
                    self.flush(key);
                }
                if events & (libc::EPOLLIN | libc::EPOLLHUP | libc::EPOLLERR) as u32 != 0 {
                    self.read(key);
                }
            }
            if (n as usize) < evs.len() {
                break;
            }
        }
        let ids: Vec<u64> = self.map.keys().copied().collect();
        ids.into_iter()
            .filter_map(|key| self.parse(key, me))
            .collect()
    }

    fn requests_open(&self) -> usize {
        self.map.values().filter(|c| !c.listening).count()
    }

    fn accept(&mut self) {
        loop {
            let s = match self.listener.accept() {
                Ok((s, _)) => s,
                // A client that gave up while queued, or a signal: the next one may be fine.
                Err(e)
                    if e.kind() == std::io::ErrorKind::Interrupted
                        || e.raw_os_error() == Some(libc::ECONNABORTED) =>
                {
                    continue;
                }
                Err(e) if matches!(e.raw_os_error(), Some(libc::EMFILE | libc::ENFILE)) => {
                    self.refuse_pending(&e);
                    return;
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        tracing::warn!("IPC: accept: {e}");
                    }
                    return;
                }
            };
            if self.requests_open() >= MAX_CONNS {
                self.expire_idle();
            }
            if self.requests_open() >= MAX_CONNS || s.set_nonblocking(true).is_err() {
                continue;
            }
            let key = self.next;
            self.next += 1;
            let fd = s.as_raw_fd();
            if ep_ctl(&self.ep, libc::EPOLL_CTL_ADD, fd, libc::EPOLLIN as u32, key).is_ok() {
                let last = Instant::now();
                self.map.insert(
                    key,
                    Conn {
                        s,
                        buf: Vec::new(),
                        need: 1,
                        out: Vec::new(),
                        busy: false,
                        eof: false,
                        closing: false,
                        listening: false,
                        last,
                    },
                );
            }
        }
    }

    /// Out of descriptors, the queued clients cannot be served, yet left in the backlog they
    /// keep the level-triggered listener readable and the main loop spinning. As libuv's
    /// `uv__emfile_trick` (and libev's documented `/dev/null` trick) does: free the reserve,
    /// accept every queued connection and close it at once, so its client reads EOF, then
    /// take the reserve again. Without one (another thread took the freed descriptor) the
    /// clients stay queued and the listener stays readable: as in libuv the loop wakes again,
    /// and [`Conns::restore_reserve`] takes the reserve back on the first wakeup a descriptor
    /// is free, so the state ends by itself.
    fn refuse_pending(&mut self, e: &std::io::Error) {
        if self.reserve.take().is_none() {
            return;
        }
        tracing::warn!("IPC: {e}; refusing the queued clients");
        loop {
            match self.listener.accept() {
                Ok(_) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::Interrupted
                        || e.raw_os_error() == Some(libc::ECONNABORTED) => {}
                Err(_) => break,
            }
        }
        self.restore_reserve();
        if self.reserve.is_none() {
            tracing::warn!("IPC: no descriptor for the reserve; queued clients wait for one");
        }
    }

    /// Takes the reserve again if it was lost: on every wakeup and whenever a connection
    /// closes (its descriptor is free).
    fn restore_reserve(&mut self) {
        if self.reserve.is_none() {
            self.reserve = reserve_fd().ok();
        }
    }

    /// Closes request connections idle for [`IDLE`] (a client that connected and sent
    /// nothing), to make room for a new one.
    fn expire_idle(&mut self) {
        let idle: Vec<u64> = self
            .map
            .iter()
            .filter(|(_, c)| !c.listening && !c.busy && c.last.elapsed() >= IDLE)
            .map(|(k, _)| *k)
            .collect();
        for k in idle {
            self.close(k);
        }
    }

    fn read(&mut self, key: u64) {
        let Some(c) = self.map.get_mut(&key) else {
            return;
        };
        let mut chunk = [0u8; 4096];
        let mut broken = false;
        loop {
            match c.s.read(&mut chunk) {
                Ok(0) => {
                    c.eof = true;
                    break;
                }
                // A listener's client sends nothing that matters.
                Ok(_) if c.listening => {}
                Ok(n) => {
                    c.buf.extend_from_slice(&chunk[..n]);
                    c.last = Instant::now();
                    if c.buf.len() > MAX_REQUEST {
                        broken = true;
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => {
                    broken = true;
                    break;
                }
            }
        }
        if broken {
            self.close(key);
        } else if c.eof {
            // A half-closed client still gets the answer to what it sent; a listener's client
            // that went away ends its stream. Stop watching for more input either way.
            if c.listening {
                self.close(key);
            } else if !c.busy && c.buf.len() < c.need {
                self.finish(key);
            } else {
                let events = if c.out.is_empty() {
                    0
                } else {
                    libc::EPOLLOUT as u32
                };
                let _ = ep_ctl(&self.ep, libc::EPOLL_CTL_MOD, c.s.as_raw_fd(), events, key);
            }
        }
    }

    /// The next complete request on `key`, unless one is still being answered. Parses again
    /// only once enough bytes arrived for the field that was incomplete last time, so a
    /// request that trickles in is not decoded again for every chunk.
    fn parse(&mut self, key: u64, me: &Rc<RefCell<Conns>>) -> Option<Request> {
        let listeners = self.map.values().filter(|c| c.listening).count();
        let c = self.map.get_mut(&key)?;
        if c.busy || c.listening || c.buf.len() < c.need {
            if c.eof && !c.busy && !c.listening {
                self.finish(key);
            }
            return None;
        }
        let mut r = Rd::new(&c.buf);
        let Some(cmd) = r.command() else {
            c.need = r.need;
            if c.need > MAX_REQUEST {
                self.close(key);
            } else if c.eof {
                self.finish(key);
            }
            return None;
        };
        let used = r.p;
        c.buf.drain(..used);
        c.need = 1;
        c.busy = true;
        let gone = c.eof;
        let call = |target, function, args, op| Call {
            target,
            function,
            args,
            op,
        };
        let (call, reply) = match cmd {
            Cmd::Kill => (
                call(String::new(), String::new(), Vec::new(), Op::Kill),
                Reply::Close,
            ),
            // An unknown command: its payload can't be skipped, so drop the connection.
            Cmd::Other => {
                self.close(key);
                return None;
            }
            Cmd::Query(target, name) => (
                call(target, name.clone(), Vec::new(), Op::Query),
                Reply::Encode(name),
            ),
            Cmd::Call(t, f, a) => (call(t, f, a, Op::Call), Reply::Encode(String::new())),
            Cmd::Prop(t, p) => (
                call(t, p, Vec::new(), Op::Prop),
                Reply::Encode(String::new()),
            ),
            // A listener whose client already went away is not registered.
            Cmd::Listen(t, sig) => {
                if listeners >= MAX_LISTENERS || gone {
                    self.close(key);
                    return None;
                }
                let l = Listener {
                    conns: Rc::downgrade(me),
                    conn: key,
                };
                (
                    call(t, sig, Vec::new(), Op::Listen(l)),
                    Reply::Encode(String::new()),
                )
            }
        };
        Some(Request {
            conn: key,
            call,
            reply,
        })
    }

    /// Queues `bytes` for `key` and writes what the socket takes now; false if it is gone.
    fn send(&mut self, key: u64, bytes: &[u8]) -> bool {
        let Some(c) = self.map.get_mut(&key) else {
            return false;
        };
        if !c.out.is_empty() && c.out.len() + bytes.len() > MAX_PENDING_OUT {
            self.close(key);
            return false;
        }
        c.out.extend_from_slice(bytes);
        self.flush(key)
    }

    fn flush(&mut self, key: u64) -> bool {
        let Some(c) = self.map.get_mut(&key) else {
            return false;
        };
        let was_waiting = c.out.len();
        while !c.out.is_empty() {
            match c.s.write(&c.out) {
                Ok(n) if n > 0 => {
                    c.out.drain(..n);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                _ => {
                    self.close(key);
                    return false;
                }
            }
        }
        if c.closing && c.out.is_empty() {
            self.close(key);
            return true;
        }
        // Watch for room while output waits; input only while the client may still send.
        let mut events = if c.eof { 0 } else { libc::EPOLLIN as u32 };
        if !c.out.is_empty() {
            events |= libc::EPOLLOUT as u32;
        }
        if was_waiting != c.out.len() || !c.out.is_empty() {
            let _ = ep_ctl(&self.ep, libc::EPOLL_CTL_MOD, c.s.as_raw_fd(), events, key);
        }
        true
    }

    /// Closes `key` once the answers already queued for it are written: a half-closed client
    /// still reads them.
    fn finish(&mut self, key: u64) {
        match self.map.get_mut(&key) {
            Some(c) if !c.out.is_empty() => {
                c.closing = true;
                let (fd, out) = (c.s.as_raw_fd(), libc::EPOLLOUT as u32);
                let _ = ep_ctl(&self.ep, libc::EPOLL_CTL_MOD, fd, out, key);
            }
            Some(_) => self.close(key),
            None => {}
        }
    }

    fn close(&mut self, key: u64) {
        if let Some(c) = self.map.remove(&key) {
            let _ = ep_ctl(&self.ep, libc::EPOLL_CTL_DEL, c.s.as_raw_fd(), 0, key);
            let _ = c.s.shutdown(std::net::Shutdown::Both);
            drop(c);
            self.restore_reserve();
        }
    }
}

/// The wire answer to `call`; `name` is the member `ipc show` asked about.
fn encode(call: &Call, name: &str, result: CallResult) -> Vec<u8> {
    let mut out = Vec::new();
    match call.op {
        Op::Query => match result {
            CallResult::Targets(v) if call.target.is_empty() => {
                out.push(4);
                out.extend((v.len() as u32).to_be_bytes());
                v.iter().for_each(|t| put_target(&mut out, t));
            }
            CallResult::Targets(v) => match v.first() {
                None => out.push(2),
                Some(t) if name.is_empty() => {
                    out.push(5);
                    put_target(&mut out, t);
                }
                Some(t) => {
                    if let Some(f) = t.functions.iter().find(|f| f.name == name) {
                        out.push(6);
                        put_fn(&mut out, f);
                    } else if let Some(p) = t.properties.iter().find(|p| p.0 == name) {
                        out.push(7);
                        put_pair(&mut out, p);
                    } else {
                        out.push(3);
                    }
                }
            },
            _ => out.push(2),
        },
        Op::Call | Op::Kill => match result {
            CallResult::Done(v) => {
                out.push(5);
                out.push(v.is_none() as u8);
                put_str(&mut out, v.as_deref().unwrap_or(""));
            }
            CallResult::ArgParse(def, count, index) => {
                out.push(4);
                put_fn(&mut out, &def);
                out.push(count as u8);
                out.push(index);
            }
            CallResult::Error(e) => {
                out.push(CALL_THREW);
                put_str(&mut out, &e);
            }
            CallResult::TargetNotFound => out.push(2),
            CallResult::EntryNotFound => out.push(3),
            _ => out.push(1),
        },
        Op::Prop => match result {
            CallResult::Value(v) => {
                out.push(4);
                put_str(&mut out, &v);
            }
            CallResult::TargetNotFound => out.push(2),
            CallResult::EntryNotFound => out.push(3),
            _ => out.push(1),
        },
        // `Listening` writes nothing: emissions follow as `SignalResponse`s.
        Op::Listen(_) => match result {
            CallResult::Listening => {}
            CallResult::TargetNotFound => out.push(2),
            CallResult::EntryNotFound => out.push(3),
            _ => out.push(1),
        },
    }
    out
}

enum Cmd {
    Kill,
    /// QueryMetadata: (target, member); empty strings list everything.
    Query(String, String),
    Call(String, String, Vec<String>),
    /// SignalListen: (target, signal).
    Listen(String, String),
    /// StringPropRead: (target, property).
    Prop(String, String),
    /// An unknown command: answered with an invalid (monostate) response.
    Other,
}

pub(crate) struct Rd<'a> {
    pub(crate) b: &'a [u8],
    pub(crate) p: usize,
    /// After a failed read: the input length the failed field needs.
    pub(crate) need: usize,
}

impl Rd<'_> {
    pub(crate) fn new(b: &[u8]) -> Rd<'_> {
        Rd { b, p: 0, need: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Option<&[u8]> {
        let Some(s) = self.b.get(self.p..self.p.saturating_add(n)) else {
            self.need = self.p.saturating_add(n);
            return None;
        };
        self.p += n;
        Some(s)
    }
    pub(crate) fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    pub(crate) fn string(&mut self) -> Option<String> {
        let n = self.u32()?;
        if n == u32::MAX {
            return Some(String::new());
        }
        let units: Vec<u16> = self
            .take(n as usize)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
    fn command(&mut self) -> Option<Cmd> {
        Some(match self.take(1)?[0] {
            1 => Cmd::Kill,
            2 => {
                let t = self.string()?;
                let n = self.string()?;
                Cmd::Query(t, n)
            }
            3 => {
                let (t, f) = (self.string()?, self.string()?);
                let n = self.u32()?;
                let mut a = Vec::new();
                for _ in 0..n {
                    a.push(self.string()?);
                }
                Cmd::Call(t, f, a)
            }
            4 => {
                let (t, n) = (self.string()?, self.string()?);
                Cmd::Listen(t, n)
            }
            5 => {
                let (t, n) = (self.string()?, self.string()?);
                Cmd::Prop(t, n)
            }
            _ => Cmd::Other,
        })
    }
}

pub fn put_str(out: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    out.extend(((u.len() * 2) as u32).to_be_bytes());
    for c in u {
        out.extend(c.to_be_bytes());
    }
}

/// `$XDG_RUNTIME_DIR/rustshell` (QsPaths::baseRunDir).
pub(crate) fn base_run_dir() -> std::path::PathBuf {
    let run = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            // SAFETY: getuid has no preconditions and always succeeds.
            format!("/run/user/{}", unsafe { libc::getuid() })
        });
    Path::new(&run).join("rustshell")
}

/// Display an instance was launched on, as quickshell's `getDisplayConnection`.
pub(crate) fn display_connection() -> String {
    match (std::env::var("WAYLAND_DISPLAY"), std::env::var("DISPLAY")) {
        (Ok(w), _) if !w.is_empty() => format!("wayland/{w}"),
        (_, Ok(x)) if !x.is_empty() => format!("x11/{x}"),
        _ => "unk".into(),
    }
}

/// `QFileInfo::absoluteFilePath`: absolute and lexically cleaned (`.`, `..`, `//`), symlinks
/// kept. The md5 of this string is the instance's `by-path` id, so server and client must agree.
pub(crate) fn clean_absolute(p: &Path) -> Option<String> {
    use std::path::Component;
    let abs = std::path::absolute(p).ok()?;
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for c in abs.components() {
        match c {
            Component::Normal(s) => parts.push(s),
            Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for s in parts {
        out.push('/');
        out.push_str(&s.to_string_lossy());
    }
    if out.is_empty() {
        out.push('/');
    }
    Some(out)
}

static INSTANCE: std::sync::OnceLock<(String, std::time::Duration)> = std::sync::OnceLock::new();

/// This process's instance id and launch time, fixed at first use (startup) or by
/// [`resume_instance`].
fn instance() -> &'static (String, std::time::Duration) {
    INSTANCE.get_or_init(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let id = format!(
            "{}{}",
            base36(u64::from(std::process::id())),
            base36(now.as_secs())
        );
        (id, now)
    })
}

/// Start this instance's log (`log.qslog` / `log.log` in its run directory) for `config`
/// (shell.qml), as Quickshell does at launch before loading the config. With `resumed` (this
/// image goes on as the instance that re-executed itself, [`resume_instance`]) that
/// instance's log is continued; `inherited` is its detailed log, kept open across the exec.
pub fn start_logging(config: &Path, resumed: bool, inherited: Option<crate::rilog::Inherited>) {
    let Some(config) = clean_absolute(config) else {
        return;
    };
    let path_id = md5_hex(config.as_bytes());
    let dir = base_run_dir().join("by-id").join(&instance().0);
    crate::rilog::start_instance_log(&config, &path_id, &path_id, &dir, resumed, inherited);
}

fn base36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = Vec::new();
    loop {
        s.push(D[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    String::from_utf8(s).unwrap_or_default()
}

pub(crate) use qml::builtins::md5_hex;

/// `ipc call ri-debug dump` (the caller checks no `IpcHandler` claims `ri-debug`): any
/// process of the user can send it, so it is answered only under `RI_DEBUG=dump`.
pub fn is_debug_dump(c: &Call, opts: &qml::debug::Opts) -> bool {
    opts.switches.contains(qml::debug::Switch::DUMP)
        && matches!(c.op, Op::Call)
        && c.target == "ri-debug"
        && c.function == "dump"
        && c.args.is_empty()
}

/// Writes the diagnostics file `name` into this instance's run directory
/// (`$XDG_RUNTIME_DIR/rustshell/by-id/<id>`, private; never through a symlink) and returns
/// its path.
pub fn write_debug_file(name: &str, data: &[u8]) -> Option<std::path::PathBuf> {
    write_private(
        &base_run_dir().join("by-id").join(&instance().0),
        name,
        data,
    )
}

fn write_private(dir: &Path, name: &str, data: &[u8]) -> Option<std::path::PathBuf> {
    let r = qml::privfs::Dir::open(dir, true).and_then(|d| d.write(name.as_ref(), data));
    match r {
        Ok(()) => Some(dir.join(name)),
        Err(e) => {
            tracing::warn!("{}/{name}: {e}", dir.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn md5() {
        assert_eq!(super::md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            super::md5_hex(b"/home/dar1k/.config/rustshell/rusty-impulse/shell.qml").len(),
            32
        );
    }

    #[test]
    fn debug_dump_needs_switch() {
        let c = super::Call {
            target: "ri-debug".into(),
            function: "dump".into(),
            args: vec![],
            op: super::Op::Call,
        };
        let o = qml::debug::Opts::parse;
        assert!(!super::is_debug_dump(&c, &o("")));
        assert!(!super::is_debug_dump(&c, &o("dlcheck")));
        assert!(super::is_debug_dump(&c, &o("dump")));
    }

    #[test]
    fn debug_files_never_follow_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("run/rustshell/by-id/x");
        std::fs::create_dir_all(&dir).unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("ri-tree.txt")).unwrap();
        let p = super::write_private(&dir, "ri-tree.txt", b"tree").unwrap();
        assert_eq!(p, dir.join("ri-tree.txt"));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "tree");
    }

    fn server() -> (super::Server, tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc.sock");
        let l = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let s = super::Server::new(l, super::Registration::default()).unwrap();
        (s, dir, path)
    }

    fn call_bytes(target: &str, function: &str) -> Vec<u8> {
        let mut b = vec![3];
        super::put_str(&mut b, target);
        super::put_str(&mut b, function);
        b.extend(0u32.to_be_bytes());
        b
    }

    fn read_exact(c: &mut std::os::unix::net::UnixStream, n: usize) -> Vec<u8> {
        use std::io::Read;
        let mut b = vec![0; n];
        c.read_exact(&mut b).unwrap();
        b
    }

    /// Whether the server's epoll fd is readable within `ms` (-1: blocks until it is), as the
    /// main loop's poll(2) sees it.
    fn readable(s: &super::Server, ms: i32) -> bool {
        let mut p = libc::pollfd {
            fd: s.fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            // SAFETY: one valid pollfd.
            let n = unsafe { libc::poll(&mut p, 1, ms) };
            if n >= 0 {
                return n > 0;
            }
            let e = std::io::Error::last_os_error();
            assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
        }
    }

    /// Sleeps in poll(2) until the server has something to do, as the main loop does.
    fn wait(s: &super::Server) {
        assert!(readable(s, -1));
    }

    /// The next batch of requests, served as the main loop does: [`super::Server::requests`]
    /// again while it returns some (a pipelined request follows its answered predecessor
    /// without a new wakeup), otherwise once the epoll fd is readable.
    fn next(s: &super::Server) -> Vec<super::Request> {
        loop {
            let reqs = s.requests();
            if !reqs.is_empty() {
                return reqs;
            }
            wait(s);
        }
    }

    /// Requests sent before the main loop runs wait in the backlog; pipelined ones come out
    /// one at a time, each after the previous one was answered, and every answer is written.
    #[test]
    fn requests_are_served_in_order_on_the_main_thread() {
        use std::io::Write;
        let (server, _dir, path) = server();
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let mut both = call_bytes("t", "a");
        both.extend(call_bytes("t", "b"));
        c.write_all(&both).unwrap();
        let mut seen = Vec::new();
        while seen.len() < 2 {
            let reqs = next(&server);
            assert!(reqs.len() <= 1, "a pipelined request skipped the queue");
            for r in reqs {
                seen.push(r.call.function.clone());
                let v = r.call.function.to_uppercase();
                server.reply(r, super::CallResult::Done(Some(v)));
            }
        }
        assert_eq!(seen, ["a", "b"]);
        let mut want = Vec::new();
        for v in ["A", "B"] {
            want.extend([5, 0]);
            super::put_str(&mut want, v);
        }
        assert_eq!(read_exact(&mut c, want.len()), want);
    }

    /// A request that trickles in is parsed again only once the incomplete field can be
    /// complete, and one announcing more than `MAX_REQUEST` closes the connection at once.
    #[test]
    fn partial_and_oversized_requests() {
        use std::io::{Read, Write};
        let (server, _dir, path) = server();
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let b = call_bytes("target", "f");
        c.write_all(&b[..7]).unwrap();
        // Each wakeup is the listener's (the accept) or the connection's (its bytes).
        while server.conns.borrow().map.values().next().map(|k| k.need) != Some(1 + 4 + 12) {
            wait(&server);
            assert!(server.requests().is_empty());
        }
        c.write_all(&b[7..]).unwrap();
        let mut reqs = next(&server);
        assert_eq!(reqs.len(), 1);
        let r = reqs.pop().unwrap();
        assert_eq!(
            (r.call.target.as_str(), r.call.function.as_str()),
            ("target", "f")
        );

        let mut big = std::os::unix::net::UnixStream::connect(&path).unwrap();
        big.write_all(&[3]).unwrap();
        big.write_all(&(u32::MAX - 1).to_be_bytes()).unwrap();
        // Served until its client reads EOF: the server closed it.
        let got = serve_until(&server, "", move || {
            let mut got = vec![0u8; 1];
            let n = big.read(&mut got).unwrap_or(0);
            got.truncate(n);
            got
        });
        assert!(got.is_empty(), "an oversized request was answered");
        assert_eq!(
            server.conns.borrow().map.len(),
            1,
            "an oversized request kept its connection"
        );
    }

    /// Request connections beyond `MAX_CONNS` are closed; signal listeners do not count.
    #[test]
    fn connections_beyond_cap_are_closed() {
        use std::io::Read;
        let (server, _dir, path) = server();
        let held: Vec<_> = (0..super::MAX_CONNS)
            .map(|_| std::os::unix::net::UnixStream::connect(&path).unwrap())
            .collect();
        let mut extra = std::os::unix::net::UnixStream::connect(&path).unwrap();
        // One wakeup accepts the whole backlog.
        assert!(readable(&server, 0));
        server.requests();
        assert_eq!(extra.read(&mut [0u8; 1]).expect("served over the cap"), 0);
        assert_eq!(server.conns.borrow().map.len(), super::MAX_CONNS);
        drop(held);
        // Every closed connection keeps the epoll fd readable until it is handled.
        while !server.conns.borrow().map.is_empty() {
            wait(&server);
            server.requests();
        }
    }

    /// At the cap, a request connection that sent nothing for `IDLE` gives its place to a new
    /// client, and a younger one keeps it. Its age wakes nobody: it is looked at when the new
    /// client arrives.
    #[test]
    fn an_idle_connection_gives_way_at_the_cap() {
        use std::io::Read;
        let (server, _dir, path) = server();
        let mut held: Vec<_> = (0..super::MAX_CONNS)
            .map(|_| std::os::unix::net::UnixStream::connect(&path).unwrap())
            .collect();
        assert!(readable(&server, 0));
        server.requests();
        assert_eq!(server.conns.borrow().map.len(), super::MAX_CONNS);
        // The first client has been silent for IDLE.
        let old = *server.conns.borrow().map.keys().min().unwrap();
        server.conns.borrow_mut().map.get_mut(&old).unwrap().last =
            std::time::Instant::now().checked_sub(super::IDLE).unwrap();
        assert!(!readable(&server, 0), "an old connection is no event");

        let extra = std::os::unix::net::UnixStream::connect(&path).unwrap();
        wait(&server);
        server.requests();
        assert_eq!(held[0].read(&mut [0u8; 1]).expect("idle client closed"), 0);
        {
            let c = server.conns.borrow();
            assert!(!c.map.contains_key(&old));
            assert_eq!(
                c.map.len(),
                super::MAX_CONNS,
                "the new client has the place"
            );
        }
        // The new client was not the one closed.
        extra.set_nonblocking(true).unwrap();
        let e = (&extra).read(&mut [0u8; 1]).expect_err("new client closed");
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
    }

    /// Out of descriptors, a queued client is accepted with the reserve and closed (it reads
    /// EOF) instead of staying in the backlog, where it would keep the epoll fd readable and
    /// the level-triggered main loop spinning; the reserve is back for the next one, and
    /// clients are served again once descriptors are free.
    #[test]
    fn queued_clients_are_refused_when_out_of_descriptors() {
        use std::io::{Read, Write};
        // RLIMIT_NOFILE is process-wide: lower it in a child running only this test.
        const CHILD: &str = "RI_IPC_EMFILE_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "ipc::tests::queued_clients_are_refused_when_out_of_descriptors",
                    "--exact",
                    "--test-threads=1",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            let log = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(out.status.success() && log.contains("1 passed"), "{log}");
            return;
        }
        // The server already closed it: the read returns EOF at once.
        let refused = |c: &mut std::os::unix::net::UnixStream| {
            assert_eq!(
                c.read(&mut [0u8; 1]).expect("client left in the backlog"),
                0
            );
        };
        let (server, _dir, path) = server();
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `lim` is a valid rlimit to fill.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) }, 0);
        lim.rlim_cur = lim.rlim_max.min(256);
        // SAFETY: `lim` is a valid rlimit to set.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) }, 0);
        let mut fill = Vec::new();
        loop {
            match std::fs::File::open("/dev/null") {
                Ok(f) => fill.push(f),
                Err(e) if e.raw_os_error() == Some(libc::EMFILE) => break,
                Err(e) => panic!("{e}"),
            }
        }
        assert!(readable(&server, 0));
        assert!(server.requests().is_empty());
        refused(&mut c);
        assert!(!readable(&server, 0), "the listener stays readable");

        drop(fill.pop());
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        assert!(std::fs::File::open("/dev/null").is_err());
        assert!(server.requests().is_empty());
        refused(&mut c);
        assert!(!readable(&server, 0), "the reserve was not taken again");

        // Another thread takes the descriptor the reserve frees: the queued client waits, but
        // the listener stays readable, and the first wakeup with a descriptor free again
        // takes the reserve back and refuses it, with no connection to wake the loop.
        drop(fill.pop());
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        drop(server.conns.borrow_mut().reserve.take());
        fill.push(std::fs::File::open("/dev/null").unwrap());
        assert!(server.conns.borrow().map.is_empty());
        assert!(server.requests().is_empty());
        assert!(server.conns.borrow().reserve.is_none());
        c.set_nonblocking(true).unwrap();
        let e = c
            .read(&mut [0u8; 1])
            .expect_err("refused without a reserve");
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
        c.set_nonblocking(false).unwrap();
        drop(fill.pop());
        assert!(readable(&server, 0), "nothing wakes the loop again");
        assert!(server.requests().is_empty());
        refused(&mut c);
        assert!(!readable(&server, 0), "the reserve was not taken back");
        assert!(server.conns.borrow().reserve.is_some());

        drop(fill);
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        c.write_all(&call_bytes("t", "f")).unwrap();
        let r = next(&server).pop().unwrap();
        server.reply(r, super::CallResult::Done(Some("ok".into())));
        let mut want = vec![5, 0];
        super::put_str(&mut want, "ok");
        assert_eq!(read_exact(&mut c, want.len()), want);
    }

    /// A listener streams emissions until its client goes away or the stream is closed.
    #[test]
    fn listeners_stream_until_their_client_goes() {
        use std::io::{Read, Write};
        let (server, _dir, path) = server();
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let mut b = vec![4];
        super::put_str(&mut b, "t");
        super::put_str(&mut b, "ping");
        c.write_all(&b).unwrap();
        let r = next(&server).pop().unwrap();
        let super::Op::Listen(l) = &r.call.op else {
            panic!("not a listen request")
        };
        let l = l.clone();
        server.reply(r, super::CallResult::Listening);
        assert!(l.alive() && l.send("v"));
        let mut want = vec![4];
        super::put_str(&mut want, "v");
        assert_eq!(read_exact(&mut c, want.len()), want);
        l.close();
        assert!(!l.alive());
        assert_eq!(c.read(&mut [0u8; 1]).unwrap(), 0, "closed stream left open");

        let mut c2 = std::os::unix::net::UnixStream::connect(&path).unwrap();
        c2.write_all(&b).unwrap();
        let r = next(&server).pop().unwrap();
        let super::Op::Listen(l2) = &r.call.op else {
            panic!("not a listen request")
        };
        let l2 = l2.clone();
        server.reply(r, super::CallResult::Listening);
        drop(c2);
        // The client's hangup keeps the epoll fd readable until it is handled.
        while l2.alive() {
            wait(&server);
            server.requests();
        }
    }

    /// Serves `server` as the main loop does, answering every request with `answer`, until
    /// `client` (run on its own thread) returns; the main loop's wait also wakes for the
    /// client's end, which is what ends the serving.
    fn serve_until(
        server: &super::Server,
        answer: &str,
        client: impl FnOnce() -> Vec<u8> + Send + 'static,
    ) -> Vec<u8> {
        let (done_tx, done_rx) = std::os::unix::net::UnixStream::pair().unwrap();
        let client = std::thread::spawn(move || {
            let got = client();
            drop(done_tx);
            got
        });
        loop {
            // Until it returns nothing, as the main loop does: the call after an answer is
            // what closes a half-closed client's connection once that answer is written.
            loop {
                let reqs = server.requests();
                if reqs.is_empty() {
                    break;
                }
                for r in reqs {
                    server.reply(r, super::CallResult::Done(Some(answer.into())));
                }
            }
            let mut p = [server.fd().as_raw_fd(), done_rx.as_raw_fd()].map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
            // SAFETY: two valid pollfds, no timeout.
            let n = unsafe { libc::poll(p.as_mut_ptr(), 2, -1) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
                continue;
            }
            if p[1].revents != 0 {
                return client.join().unwrap();
            }
        }
    }

    fn answer_bytes(v: &str) -> Vec<u8> {
        let mut want = vec![5, 0];
        super::put_str(&mut want, v);
        want
    }

    /// A client that half-closes after its request (`shutdown(SHUT_WR)`, as `socat` does at
    /// the end of its input) still reads the whole answer, also one larger than the socket
    /// buffer that is written out over several wakeups.
    #[test]
    fn half_closed_client_gets_the_whole_answer() {
        use std::io::{Read, Write};
        let (server, _dir, path) = server();
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        c.write_all(&call_bytes("t", "big")).unwrap();
        c.shutdown(std::net::Shutdown::Write).unwrap();
        let big = "x".repeat(600_000);
        let got = serve_until(&server, &big, move || {
            let mut got = Vec::new();
            c.read_to_end(&mut got).unwrap();
            got
        });
        assert_eq!(got.len(), answer_bytes(&big).len(), "answer cut short");
        assert_eq!(got, answer_bytes(&big));
        assert!(
            server.conns.borrow().map.is_empty(),
            "finished connection kept"
        );
    }

    /// One answer larger than `MAX_PENDING_OUT` is delivered: the cap bounds output piling up
    /// for a client that does not read, not the size of a single reply.
    #[test]
    fn an_answer_larger_than_the_output_cap_is_delivered() {
        use std::io::{Read, Write};
        let (server, _dir, path) = server();
        let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
        c.write_all(&call_bytes("t", "huge")).unwrap();
        let huge = "y".repeat(super::MAX_PENDING_OUT);
        let n = answer_bytes(&huge).len();
        let got = serve_until(&server, &huge, move || {
            let mut got = Vec::new();
            let _ = c.take(n as u64).read_to_end(&mut got);
            got
        });
        assert_eq!(got.len(), n, "answer dropped or cut short");
        assert_eq!(got, answer_bytes(&huge));
    }

    /// Startup keeps only the newest dead instance of the config (for `qs log`), removing the
    /// older ones with their `by-shell` and `by-pid` links, and drops links to removed dirs.
    #[test]
    fn dead_instances_are_pruned_but_the_newest() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path();
        let shell = base.join("by-shell/p");
        std::fs::create_dir_all(&shell).unwrap();
        std::fs::create_dir_all(base.join("by-pid")).unwrap();
        for (i, id) in ["old", "mid", "new", "me"].iter().enumerate() {
            let dir = base.join("by-id").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            let lock = std::fs::File::create(dir.join("instance.lock")).unwrap();
            let at = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1000 + i as u64);
            lock.set_modified(at).unwrap();
            std::fs::write(dir.join("log.qslog"), "x").unwrap();
            std::os::unix::fs::symlink(&dir, shell.join(id)).unwrap();
            std::os::unix::fs::symlink(&dir, base.join("by-pid").join(format!("{i}"))).unwrap();
        }
        std::os::unix::fs::symlink(base.join("by-id/gone"), shell.join("gone")).unwrap();
        super::prune_dead(base, &shell, "me");
        let ids = |d: &str| {
            let mut v: Vec<String> = std::fs::read_dir(base.join(d))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        assert_eq!(ids("by-id"), ["me", "new"]);
        assert_eq!(ids("by-shell/p"), ["me", "new"]);
        assert_eq!(ids("by-pid"), ["2", "3"]);
    }

    #[test]
    fn relink_replaces_a_link() {
        let t = tempfile::tempdir().unwrap();
        let link = t.path().join("l");
        super::relink(&link, Path::new("/a")).unwrap();
        super::relink(&link, Path::new("/b")).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("/b"));
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 1);
    }

    #[test]
    fn clean_absolute() {
        let c = |p: &str| super::clean_absolute(std::path::Path::new(p)).unwrap();
        assert_eq!(c("/a/./b//c/../shell.qml"), "/a/b/shell.qml");
        assert_eq!(c("/a/b/"), "/a/b");
        assert_eq!(c("/.."), "/");
    }
}
